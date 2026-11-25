use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{
    self,
    Receiver,
    RecvTimeoutError,
    Sender,
    SyncSender,
    TryRecvError,
    TrySendError,
};
use std::thread;
use std::time::{Duration, Instant};

use libcamera::{
    camera::{ActiveCamera, CameraConfigurationStatus},
    camera_manager::CameraManager,
    framebuffer_allocator::{FrameBuffer as CameraFrameBuffer, FrameBufferAllocator},
    framebuffer_map::MemoryMappedFrameBuffer,
    geometry::Size,
    pixel_format::PixelFormat,
    request::{Request, RequestStatus, ReuseFlag},
    stream::{Stream, StreamRole},
};

use crate::{
    apply_adjustments,
    capture_photo_max_resolution_with_manager,
    preferred_camera_id,
    preview_dimensions,
    refresh_preview_capabilities,
    spawn_video_recorder,
    timestamp,
    tr,
    trf,
    video_library_dir,
    AdjustmentProfile,
    CameraConfig,
    OwnedFrame,
    RecorderHandle,
    PREVIEW_FRAMERATE,
};

const RECORDING_DROP_STATUS_INTERVAL: u32 = PREVIEW_FRAMERATE * 2;
const RETRY_DELAYS_MS: [u64; 6] = [200, 400, 800, 1_600, 3_000, 5_000];
const MANAGER_RECREATE_AFTER_FAILURES: usize = 12;
const FRAME_STALL_TIMEOUT: Duration = Duration::from_secs(3);
const CONSECUTIVE_FRAME_ERROR_LIMIT: u8 = 3;

pub enum WorkerCommand {
    StartPreview,
    StopPreview,
    ApplyConfig {
        config: CameraConfig,
        restart: bool,
    },
    CapturePhoto {
        output_path: PathBuf,
    },
    StartRecording,
    StopRecording,
    Shutdown,
}

pub enum WorkerEvent {
    PreviewStarted {
        width: usize,
        height: usize,
    },
    PreviewStopped {
        reason: String,
    },
    PreviewRetrying {
        reason: String,
    },
    PreviewFrame {
        frame: OwnedFrame,
        fps: f32,
    },
    Status(String),
    PhotoFinished {
        success: bool,
        output_path: PathBuf,
        stderr: String,
        resolution: Option<(usize, usize)>,
    },
    RecordingStartFailed {
        reason: String,
    },
    RecordingFinished {
        success: bool,
        output_path: PathBuf,
        stderr: String,
    },
}

struct PreviewSession<'a> {
    camera: ActiveCamera<'a>,
    stream: Stream,
    request_rx: Receiver<Request>,
    width: usize,
    height: usize,
    stride: usize,
    profile: AdjustmentProfile,
    recorder: Option<RecorderHandle>,
    pending_recording_output: Option<PathBuf>,
    dropped_recording_frames: u32,
    last_fps: f32,
    started_at: Instant,
    fps_window_started: Instant,
    fps_window_frames: u32,
    last_request_at: Instant,
    consecutive_frame_errors: u8,
}

#[derive(Default)]
struct RetryBackoff {
    next_delay_index: usize,
    next_attempt: Option<Instant>,
}

struct RetrySchedule {
    delay: Duration,
    should_report: bool,
}

impl RetryBackoff {
    fn start_now(&mut self) {
        self.next_delay_index = 0;
        self.next_attempt = Some(Instant::now());
    }

    fn cancel(&mut self) {
        self.next_delay_index = 0;
        self.next_attempt = None;
    }

    fn mark_started(&mut self) {
        self.next_attempt = None;
    }

    fn is_due(&self) -> bool {
        self.next_attempt
            .is_some_and(|next_attempt| Instant::now() >= next_attempt)
    }

    fn schedule_after_failure(&mut self) -> RetrySchedule {
        let last_index = RETRY_DELAYS_MS.len() - 1;
        let delay_index = self.next_delay_index.min(last_index);
        let delay = Duration::from_millis(RETRY_DELAYS_MS[delay_index]);
        let should_report = self.next_delay_index <= last_index;
        self.next_delay_index = self.next_delay_index.saturating_add(1);
        self.next_attempt = Some(Instant::now() + delay);
        RetrySchedule {
            delay,
            should_report,
        }
    }

    fn wait_timeout(&self) -> Option<Duration> {
        self.next_attempt
            .map(|next_attempt| next_attempt.saturating_duration_since(Instant::now()))
    }
}

struct WorkerState {
    config: CameraConfig,
    preview_requested: bool,
}

enum ManagerCommandWait {
    Command(WorkerCommand),
    Timeout,
    Disconnected,
}

enum ManagerLoopExit {
    Recreate,
    Shutdown,
}

enum PreviewAttemptResult {
    Started,
    TransientFailure,
    FatalFailure,
}

enum PreviewStartError {
    Transient(String),
    Fatal(String),
}

fn record_transient_failure(failures: &mut usize) -> bool {
    *failures = (*failures).saturating_add(1);
    *failures >= MANAGER_RECREATE_AFTER_FAILURES
}

fn frame_stalled(elapsed_since_request: Duration) -> bool {
    elapsed_since_request >= FRAME_STALL_TIMEOUT
}

impl<'a> PreviewSession<'a> {
    fn update_profile(&mut self, config: &CameraConfig) {
        self.profile = AdjustmentProfile::new(config);
    }
}

fn emit_preview_frame(event_tx: &SyncSender<WorkerEvent>, frame: OwnedFrame, fps: f32) {
    match event_tx.try_send(WorkerEvent::PreviewFrame { frame, fps }) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {}
        Err(TrySendError::Disconnected(_)) => {}
    }
}

fn emit_status(event_tx: &SyncSender<WorkerEvent>, message: String) {
    match event_tx.try_send(WorkerEvent::Status(message)) {
        Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
    }
}

pub fn spawn_camera_worker(
    initial_config: CameraConfig,
) -> (Sender<WorkerCommand>, Receiver<WorkerEvent>) {
    let (command_tx, event_rx, _worker) = spawn_camera_worker_thread(initial_config);
    (command_tx, event_rx)
}

fn spawn_camera_worker_thread(
    initial_config: CameraConfig,
) -> (
    Sender<WorkerCommand>,
    Receiver<WorkerEvent>,
    thread::JoinHandle<()>,
) {
    let (command_tx, command_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(2);
    let worker = thread::spawn(move || camera_worker(initial_config, command_rx, event_tx));
    (command_tx, event_rx, worker)
}

fn camera_worker(
    initial_config: CameraConfig,
    command_rx: Receiver<WorkerCommand>,
    event_tx: SyncSender<WorkerEvent>,
) {
    let mut state = WorkerState {
        config: initial_config,
        preview_requested: false,
    };
    state.config.sanitize();
    let mut manager_retry = RetryBackoff::default();
    let mut preview_retry = RetryBackoff::default();
    manager_retry.start_now();

    loop {
        if manager_retry.is_due() {
            match CameraManager::new() {
                Ok(manager) => {
                    manager_retry.cancel();
                    match run_camera_manager_loop(
                        &manager,
                        &command_rx,
                        &event_tx,
                        &mut state,
                        &mut preview_retry,
                    ) {
                        ManagerLoopExit::Recreate => {
                            manager_retry.start_now();
                            continue;
                        }
                        ManagerLoopExit::Shutdown => return,
                    }
                }
                Err(error) => {
                    report_manager_retry(&mut manager_retry, &state, &event_tx, &error.to_string());
                }
            }
        }

        match wait_for_manager_command(&command_rx, &manager_retry) {
            ManagerCommandWait::Command(command) => {
                if handle_command_without_manager(
                    command,
                    &mut state,
                    &mut manager_retry,
                    &mut preview_retry,
                    &event_tx,
                ) {
                    return;
                }
            }
            ManagerCommandWait::Timeout => {}
            ManagerCommandWait::Disconnected => return,
        }
    }
}

fn wait_for_manager_command(
    command_rx: &Receiver<WorkerCommand>,
    retry: &RetryBackoff,
) -> ManagerCommandWait {
    match retry.wait_timeout() {
        Some(timeout) => match command_rx.recv_timeout(timeout) {
            Ok(command) => ManagerCommandWait::Command(command),
            Err(RecvTimeoutError::Timeout) => ManagerCommandWait::Timeout,
            Err(RecvTimeoutError::Disconnected) => ManagerCommandWait::Disconnected,
        },
        None => match command_rx.recv() {
            Ok(command) => ManagerCommandWait::Command(command),
            Err(_) => ManagerCommandWait::Disconnected,
        },
    }
}

fn report_manager_retry(
    retry: &mut RetryBackoff,
    state: &WorkerState,
    event_tx: &SyncSender<WorkerEvent>,
    error: &str,
) {
    let schedule = retry.schedule_after_failure();
    if !state.preview_requested || !schedule.should_report {
        return;
    }
    emit_status(
        event_tx,
        trf(
            "Libcamera ainda não iniciou ({error}). Nova tentativa em {delay_ms} ms.",
            &[
                ("error", error.to_string()),
                ("delay_ms", schedule.delay.as_millis().to_string()),
            ],
        ),
    );
}

fn handle_command_without_manager(
    command: WorkerCommand,
    state: &mut WorkerState,
    manager_retry: &mut RetryBackoff,
    preview_retry: &mut RetryBackoff,
    event_tx: &SyncSender<WorkerEvent>,
) -> bool {
    match command {
        WorkerCommand::StartPreview => {
            state.preview_requested = true;
            manager_retry.start_now();
            preview_retry.start_now();
            false
        }
        WorkerCommand::StopPreview => {
            state.preview_requested = false;
            manager_retry.cancel();
            preview_retry.cancel();
            let _ = event_tx.send(WorkerEvent::PreviewStopped {
                reason: tr("Preview parado."),
            });
            false
        }
        WorkerCommand::ApplyConfig {
            config: mut next_config,
            restart,
        } => {
            next_config.sanitize();
            state.config = next_config;
            if restart && state.preview_requested {
                manager_retry.start_now();
                preview_retry.start_now();
            }
            false
        }
        WorkerCommand::CapturePhoto { output_path } => {
            let _ = event_tx.send(WorkerEvent::PhotoFinished {
                success: false,
                output_path,
                stderr: tr("Libcamera indisponível para capturar a foto."),
                resolution: None,
            });
            false
        }
        WorkerCommand::StartRecording => {
            let _ = event_tx.send(WorkerEvent::RecordingStartFailed {
                reason: tr("Libcamera indisponível para iniciar a gravação."),
            });
            false
        }
        WorkerCommand::StopRecording => false,
        WorkerCommand::Shutdown => true,
    }
}

fn run_camera_manager_loop<'a>(
    manager: &'a CameraManager,
    command_rx: &Receiver<WorkerCommand>,
    event_tx: &SyncSender<WorkerEvent>,
    state: &mut WorkerState,
    retry: &mut RetryBackoff,
) -> ManagerLoopExit {
    let mut session: Option<PreviewSession<'a>> = None;
    let mut transient_failures = 0_usize;
    if state.preview_requested && retry.next_attempt.is_none() {
        retry.start_now();
    }

    loop {
        loop {
            match command_rx.try_recv() {
                Ok(command) => {
                    if handle_manager_command(
                        command,
                        manager,
                        state,
                        &mut session,
                        retry,
                        event_tx,
                    ) {
                        if let Some(mut active_session) = session.take() {
                            stop_preview_session(&mut active_session);
                        }
                        return ManagerLoopExit::Shutdown;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if let Some(mut active_session) = session.take() {
                        stop_preview_session(&mut active_session);
                    }
                    return ManagerLoopExit::Shutdown;
                }
            }
        }

        if state.preview_requested && session.is_none() && retry.is_due() {
            match attempt_preview_start(manager, state, &mut session, retry, event_tx) {
                PreviewAttemptResult::Started => {}
                PreviewAttemptResult::FatalFailure => transient_failures = 0,
                PreviewAttemptResult::TransientFailure => {
                    if record_transient_failure(&mut transient_failures) {
                        return ManagerLoopExit::Recreate;
                    }
                }
            }
        }

        let Some(active_session) = session.as_mut() else {
            match wait_for_manager_command(command_rx, retry) {
                ManagerCommandWait::Command(command) => {
                    if handle_manager_command(
                        command,
                        manager,
                        state,
                        &mut session,
                        retry,
                        event_tx,
                    ) {
                        return ManagerLoopExit::Shutdown;
                    }
                }
                ManagerCommandWait::Timeout => {}
                ManagerCommandWait::Disconnected => return ManagerLoopExit::Shutdown,
            }
            continue;
        };

        if active_session.started_at.elapsed() >= Duration::from_secs(5) {
            retry.cancel();
            transient_failures = 0;
        }
        match active_session
            .request_rx
            .recv_timeout(Duration::from_millis(20))
        {
            Ok(mut request) => {
                active_session.last_request_at = Instant::now();
                if request.status() != RequestStatus::Complete {
                    let reason = trf(
                        "O libcamera devolveu um request de preview com estado inesperado: {status}.",
                        &[("status", format!("{:?}", request.status()))],
                    );
                    interrupt_preview_for_retry(
                        &mut session,
                        &reason,
                        state,
                        retry,
                        event_tx,
                    );
                    if record_transient_failure(&mut transient_failures) {
                        return ManagerLoopExit::Recreate;
                    }
                    continue;
                }

                match process_completed_request(
                    active_session,
                    &state.config,
                    &mut request,
                    event_tx,
                ) {
                    Ok(()) => active_session.consecutive_frame_errors = 0,
                    Err(error) => {
                        active_session.consecutive_frame_errors =
                            active_session.consecutive_frame_errors.saturating_add(1);
                        if active_session.consecutive_frame_errors == 1 {
                            emit_status(
                                event_tx,
                                trf("Erro no preview: {error}", &[("error", error.clone())]),
                            );
                        }
                        if active_session.consecutive_frame_errors
                            >= CONSECUTIVE_FRAME_ERROR_LIMIT
                        {
                            let reason = trf(
                                "O preview produziu frames inválidos repetidamente: {error}",
                                &[("error", error)],
                            );
                            interrupt_preview_for_retry(
                                &mut session,
                                &reason,
                                state,
                                retry,
                                event_tx,
                            );
                            if record_transient_failure(&mut transient_failures) {
                                return ManagerLoopExit::Recreate;
                            }
                            continue;
                        }
                    }
                }

                request.reuse(ReuseFlag::REUSE_BUFFERS);
                if let Err((_, error)) = active_session.camera.queue_request(request) {
                    let reason = trf(
                        "Falha ao reenfileirar o frame da câmera: {error}",
                        &[("error", error.to_string())],
                    );
                    interrupt_preview_for_retry(
                        &mut session,
                        &reason,
                        state,
                        retry,
                        event_tx,
                    );
                    if record_transient_failure(&mut transient_failures) {
                        return ManagerLoopExit::Recreate;
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if frame_stalled(active_session.last_request_at.elapsed()) {
                    let reason = trf(
                        "O preview não recebeu frames por {seconds}s.",
                        &[("seconds", FRAME_STALL_TIMEOUT.as_secs().to_string())],
                    );
                    interrupt_preview_for_retry(
                        &mut session,
                        &reason,
                        state,
                        retry,
                        event_tx,
                    );
                    if record_transient_failure(&mut transient_failures) {
                        return ManagerLoopExit::Recreate;
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                interrupt_preview_for_retry(
                    &mut session,
                    &tr("Canal interno do preview foi encerrado."),
                    state,
                    retry,
                    event_tx,
                );
                if record_transient_failure(&mut transient_failures) {
                    return ManagerLoopExit::Recreate;
                }
            }
        }
    }
}

fn handle_manager_command<'a>(
    command: WorkerCommand,
    manager: &'a CameraManager,
    state: &mut WorkerState,
    session: &mut Option<PreviewSession<'a>>,
    retry: &mut RetryBackoff,
    event_tx: &SyncSender<WorkerEvent>,
) -> bool {
    match command {
        WorkerCommand::StartPreview => {
            state.preview_requested = true;
            stop_session_for_planned_restart(
                session,
                stop_preview_session,
                tr("Iniciando preview..."),
                event_tx,
            );
            retry.start_now();
            false
        }
        WorkerCommand::StopPreview => {
            state.preview_requested = false;
            retry.cancel();
            if let Some(mut active_session) = session.take() {
                stop_preview_session(&mut active_session);
            }
            let _ = event_tx.send(WorkerEvent::PreviewStopped {
                reason: tr("Preview parado."),
            });
            false
        }
        WorkerCommand::ApplyConfig {
            config: mut next_config,
            restart,
        } => {
            next_config.sanitize();
            state.config = next_config;
            if let Some(active_session) = session.as_mut() {
                active_session.update_profile(&state.config);
            }
            if restart && state.preview_requested {
                stop_session_for_planned_restart(
                    session,
                    stop_preview_session,
                    tr("Reiniciando o preview para aplicar a nova resolução ou preset..."),
                    event_tx,
                );
                retry.start_now();
            }
            false
        }
        WorkerCommand::CapturePhoto { output_path } => {
            if let Some(active_session) = session.as_ref() {
                if active_session.recorder.is_some()
                    || active_session.pending_recording_output.is_some()
                {
                    let _ = event_tx.send(WorkerEvent::PhotoFinished {
                        success: false,
                        output_path,
                        stderr: tr("Pare a gravação antes de tirar foto em resolução máxima."),
                        resolution: None,
                    });
                    return false;
                }
            }

            let should_restore_preview = state.preview_requested;
            if should_restore_preview {
                stop_session_for_planned_restart(
                    session,
                    stop_preview_session,
                    trf(
                        "Capturando foto em resolução máxima em {output_path}...",
                        &[("output_path", output_path.display().to_string())],
                    ),
                    event_tx,
                );
            } else if let Some(mut active_session) = session.take() {
                stop_preview_session(&mut active_session);
            }

            let result = capture_photo_max_resolution_with_manager(
                manager,
                &state.config,
                &output_path,
            );

            if should_restore_preview {
                retry.start_now();
            }

            match result {
                Ok((width, height)) => {
                    let _ = event_tx.send(WorkerEvent::PhotoFinished {
                        success: true,
                        output_path,
                        stderr: String::new(),
                        resolution: Some((width, height)),
                    });
                }
                Err(error) => {
                    let _ = event_tx.send(WorkerEvent::PhotoFinished {
                        success: false,
                        output_path,
                        stderr: error,
                        resolution: None,
                    });
                }
            }
            false
        }
        WorkerCommand::StartRecording => {
            let Some(active_session) = session.as_mut() else {
                let _ = event_tx.send(WorkerEvent::RecordingStartFailed {
                    reason: tr("Inicie o preview antes de gravar vídeo."),
                });
                return false;
            };
            if active_session.recorder.is_some()
                || active_session.pending_recording_output.is_some()
            {
                return false;
            }
            let output_path = video_library_dir().join(format!("camera-{}.mp4", timestamp()));
            active_session.pending_recording_output = Some(output_path.clone());
            emit_status(event_tx, trf(
                "Preparando gravação em {output_path}...",
                &[("output_path", output_path.display().to_string())],
            ));
            false
        }
        WorkerCommand::StopRecording => {
            let Some(active_session) = session.as_mut() else {
                return false;
            };
            active_session.pending_recording_output = None;
            if let Some(recorder) = active_session.recorder.take() {
                drop(recorder);
                emit_status(event_tx, tr("Finalizando arquivo de vídeo..."));
            }
            false
        }
        WorkerCommand::Shutdown => true,
    }
}

fn attempt_preview_start<'a>(
    manager: &'a CameraManager,
    state: &mut WorkerState,
    session: &mut Option<PreviewSession<'a>>,
    retry: &mut RetryBackoff,
    event_tx: &SyncSender<WorkerEvent>,
) -> PreviewAttemptResult {
    // Each attempt asks the live CameraManager for a fresh cameras() snapshot, so a
    // sensor registered after startup is observed without relying on crate 0.7's
    // leaking hotplug callback bridge.
    match start_preview_session(manager, &state.config) {
        Ok(active_session) => {
            let _ = event_tx.send(WorkerEvent::PreviewStarted {
                width: active_session.width,
                height: active_session.height,
            });
            emit_status(event_tx, trf(
                "Preview ativo em {width}x{height} com libcamera direto.",
                &[
                    ("width", active_session.width.to_string()),
                    ("height", active_session.height.to_string()),
                ],
            ));
            retry.mark_started();
            *session = Some(active_session);
            PreviewAttemptResult::Started
        }
        Err(PreviewStartError::Transient(error)) => {
            schedule_preview_retry(&error, state, retry, event_tx);
            PreviewAttemptResult::TransientFailure
        }
        Err(PreviewStartError::Fatal(error)) => {
            fail_preview_permanently(state, retry, event_tx, error)
        }
    }
}

fn fail_preview_permanently(
    state: &mut WorkerState,
    retry: &mut RetryBackoff,
    event_tx: &SyncSender<WorkerEvent>,
    error: String,
) -> PreviewAttemptResult {
    state.preview_requested = false;
    retry.cancel();
    let _ = event_tx.send(WorkerEvent::PreviewStopped {
        reason: trf(
            "Falha não recuperável ao configurar o preview: {error}",
            &[("error", error)],
        ),
    });
    PreviewAttemptResult::FatalFailure
}

fn schedule_preview_retry(
    error: &str,
    state: &WorkerState,
    retry: &mut RetryBackoff,
    event_tx: &SyncSender<WorkerEvent>,
) {
    if !state.preview_requested {
        retry.cancel();
        return;
    }
    let schedule = retry.schedule_after_failure();
    if schedule.should_report {
        emit_status(
            event_tx,
            trf(
                "Câmera ainda indisponível ({error}). Nova tentativa em {delay_ms} ms.",
                &[
                    ("error", error.to_string()),
                    ("delay_ms", schedule.delay.as_millis().to_string()),
                ],
            ),
        );
    }
}

fn interrupt_preview_for_retry<'a>(
    session: &mut Option<PreviewSession<'a>>,
    reason: &str,
    state: &WorkerState,
    retry: &mut RetryBackoff,
    event_tx: &SyncSender<WorkerEvent>,
) {
    if let Some(mut active_session) = session.take() {
        stop_preview_session(&mut active_session);
        let _ = event_tx.send(WorkerEvent::PreviewRetrying {
            reason: reason.to_string(),
        });
    }
    schedule_preview_retry(reason, state, retry, event_tx);
}

fn stop_session_for_planned_restart<T>(
    session: &mut Option<T>,
    mut stop_session: impl FnMut(&mut T),
    reason: String,
    event_tx: &SyncSender<WorkerEvent>,
) {
    if let Some(mut active_session) = session.take() {
        stop_session(&mut active_session);
        let _ = event_tx.send(WorkerEvent::PreviewRetrying { reason });
    }
}

fn start_preview_session<'a>(
    manager: &'a CameraManager,
    config: &CameraConfig,
) -> Result<PreviewSession<'a>, PreviewStartError> {
    let camera_id = preferred_camera_id(manager)
        .ok_or_else(|| {
            PreviewStartError::Transient(tr("O libcamera não encontrou câmeras. Se o problema persistir, verifique o sensor e o driver no Galaxy Book Setup."))
        })?;
    let camera_ref = manager
        .get(&camera_id)
        .ok_or_else(|| PreviewStartError::Transient(trf("Câmera {camera_id} não ficou acessível pelo CameraManager.", &[("camera_id", camera_id.clone())])))?;
    let mut camera = camera_ref
        .acquire()
        .map_err(|error| PreviewStartError::Transient(trf("Falha ao adquirir a câmera: {error}", &[("error", error.to_string())])))?;

    let mut configuration = camera
        .generate_configuration(&[StreamRole::ViewFinder])
        .ok_or_else(|| PreviewStartError::Fatal(tr("Não foi possível gerar a configuração padrão da câmera.")))?;
    let Some(mut stream_cfg) = configuration.get_mut(0) else {
        return Err(PreviewStartError::Fatal(tr("A configuração da câmera não retornou um stream válido.")));
    };

    let pixel_format = PixelFormat::parse("XBGR8888")
        .ok_or_else(|| PreviewStartError::Fatal(tr("XBGR8888 não está disponível neste host.")))?;
    let formats = stream_cfg.formats();
    let range = formats.range(pixel_format);
    let _ = refresh_preview_capabilities(
        formats
            .sizes(pixel_format)
            .into_iter()
            .map(|size| (size.width, size.height)),
        Some((range.max.width, range.max.height)),
    );
    stream_cfg.set_pixel_format(pixel_format);
    if let (Some(width), Some(height)) = (config.width, config.height) {
        stream_cfg.set_size(Size::new(width, height));
    }

    match configuration.validate() {
        CameraConfigurationStatus::Invalid => {
            return Err(PreviewStartError::Fatal(tr("A configuração solicitada ficou inválida depois da validação.")))
        }
        CameraConfigurationStatus::Adjusted | CameraConfigurationStatus::Valid => {}
    }

    let validated_cfg = configuration
        .get(0)
        .ok_or_else(|| PreviewStartError::Fatal(tr("Não foi possível ler o stream validado.")))?;
    if validated_cfg.get_pixel_format() != pixel_format {
        return Err(PreviewStartError::Fatal(trf(
            "A câmera não aceitou XBGR8888 para o preview; formato final: {pixel_format}.",
            &[(
                "pixel_format",
                format!("{:?}", validated_cfg.get_pixel_format()),
            )],
        )));
    }

    camera
        .configure(&mut configuration)
        .map_err(|error| PreviewStartError::Transient(trf("Falha ao configurar a câmera: {error}", &[("error", error.to_string())])))?;

    let stream_cfg = configuration
        .get(0)
        .ok_or_else(|| PreviewStartError::Fatal(tr("Não foi possível ler o stream configurado.")))?;
    let stream = stream_cfg.stream().ok_or_else(|| {
        PreviewStartError::Fatal(tr("O stream configurado não ficou disponível depois do configure()."))
    })?;
    let size = stream_cfg.get_size();
    let width = size.width as usize;
    let height = size.height as usize;
    let stride = stream_cfg.get_stride() as usize;

    let mut allocator = FrameBufferAllocator::new(&camera);
    let buffers = allocator
        .alloc(&stream)
        .map_err(|error| PreviewStartError::Transient(trf("Falha ao alocar buffers da câmera: {error}", &[("error", error.to_string())])))?;
    let buffers = buffers
        .into_iter()
        .map(|buffer| {
            MemoryMappedFrameBuffer::new(buffer)
                .map_err(|error| PreviewStartError::Transient(trf("Falha ao mapear buffer da câmera na memória: {error}", &[("error", error.to_string())])))
        })
        .collect::<Result<Vec<MemoryMappedFrameBuffer<CameraFrameBuffer>>, PreviewStartError>>()?;

    let mut requests = Vec::with_capacity(buffers.len());
    for (index, buffer) in buffers.into_iter().enumerate() {
        let mut request = camera
            .create_request(Some(index as u64))
            .ok_or_else(|| PreviewStartError::Fatal(tr("Falha ao criar uma requisição de captura.")))?;
        request
            .add_buffer(&stream, buffer)
            .map_err(|error| PreviewStartError::Fatal(trf("Falha ao anexar buffer à requisição: {error}", &[("error", error.to_string())])))?;
        requests.push(request);
    }

    let request_rx = camera.subscribe_request_completed();
    camera
        .start(None)
        .map_err(|error| PreviewStartError::Transient(trf("Falha ao iniciar a captura: {error}", &[("error", error.to_string())])))?;

    for request in requests {
        camera
            .queue_request(request)
            .map_err(|(_, error)| PreviewStartError::Transient(trf("Falha ao enfileirar requisição inicial: {error}", &[("error", error.to_string())])))?;
    }

    Ok(PreviewSession {
        camera,
        stream,
        request_rx,
        width,
        height,
        stride,
        profile: AdjustmentProfile::new(config),
        recorder: None,
        pending_recording_output: None,
        dropped_recording_frames: 0,
        last_fps: 0.0,
        started_at: Instant::now(),
        fps_window_started: Instant::now(),
        fps_window_frames: 0,
        last_request_at: Instant::now(),
        consecutive_frame_errors: 0,
    })
}

fn stop_preview_session(session: &mut PreviewSession<'_>) {
    session.pending_recording_output = None;
    session.dropped_recording_frames = 0;
    if let Some(recorder) = session.recorder.take() {
        drop(recorder);
    }
    let _ = session.camera.stop();
}

fn process_completed_request(
    session: &mut PreviewSession<'_>,
    config: &CameraConfig,
    request: &mut Request,
    event_tx: &SyncSender<WorkerEvent>,
) -> Result<(), String> {
    let framebuffer = request
        .buffer::<MemoryMappedFrameBuffer<CameraFrameBuffer>>(&session.stream)
        .ok_or_else(|| tr("O request completado não trouxe o buffer esperado."))?;
    let planes = framebuffer.data();
    let plane = planes
        .first()
        .copied()
        .ok_or_else(|| tr("O request completado não trouxe dados de imagem."))?;
    let (preview_width, preview_height) = preview_dimensions(session.width, session.height);
    let needs_full_output =
        session.recorder.is_some() || session.pending_recording_output.is_some();

    let preview_frame = if needs_full_output {
        let mut output_frame =
            OwnedFrame::from_strided_rgba(session.width, session.height, session.stride, plane)?;
        apply_adjustments(&mut output_frame, &session.profile);

        if let Some(output_path) = session.pending_recording_output.take() {
            fs::create_dir_all(video_library_dir())
                .map_err(|error| trf("Falha ao preparar a pasta da câmera: {error}", &[("error", error.to_string())]))?;
            match spawn_video_recorder(
                output_path.clone(),
                output_frame.width,
                output_frame.height,
                config.record_audio,
                &config.audio_source,
                event_tx.clone(),
            ) {
                Ok(recorder) => {
                    let backend_label = recorder.backend.ui_label();
                    session.recorder = Some(recorder);
                    session.dropped_recording_frames = 0;
                    emit_status(event_tx, trf(
                        "Gravando vídeo em {output_path} usando {backend_label}.",
                        &[
                            ("output_path", output_path.display().to_string()),
                            ("backend_label", backend_label),
                        ],
                    ));
                }
                Err(error) => {
                    let _ = event_tx.send(WorkerEvent::RecordingFinished {
                        success: false,
                        output_path,
                        stderr: error,
                    });
                }
            }
        }

        if let Some(recorder) = session.recorder.as_ref() {
            if !recorder.try_send_frame(&output_frame) {
                session.dropped_recording_frames += 1;
                if session.dropped_recording_frames == 1
                    || session.dropped_recording_frames % RECORDING_DROP_STATUS_INTERVAL == 0
                {
                    emit_status(event_tx, trf(
                        "O encoder de vídeo está atrasado; {frames} frame(s) foram ignorados para manter a gravação responsiva.",
                        &[("frames", session.dropped_recording_frames.to_string())],
                    ));
                }
            }
        }

        output_frame.scaled_nearest(preview_width, preview_height)
    } else {
        let mut preview_frame = OwnedFrame::from_strided_rgba_scaled(
            session.width,
            session.height,
            session.stride,
            plane,
            preview_width,
            preview_height,
        )?;
        apply_adjustments(&mut preview_frame, &session.profile);
        preview_frame
    };

    session.fps_window_frames += 1;
    let elapsed = session.fps_window_started.elapsed();
    if elapsed >= Duration::from_millis(400) {
        session.last_fps = session.fps_window_frames as f32 / elapsed.as_secs_f32();
        session.fps_window_started = Instant::now();
        session.fps_window_frames = 0;
    }

    emit_preview_frame(event_tx, preview_frame, session.last_fps);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn retry_backoff_caps_at_five_seconds_without_status_spam() {
        let mut retry = RetryBackoff::default();
        retry.start_now();

        let schedules = RETRY_DELAYS_MS
            .iter()
            .map(|_| retry.schedule_after_failure())
            .collect::<Vec<_>>();
        assert_eq!(
            schedules
                .iter()
                .map(|schedule| schedule.delay.as_millis())
                .collect::<Vec<_>>(),
            RETRY_DELAYS_MS
                .iter()
                .map(|delay| u128::from(*delay))
                .collect::<Vec<_>>()
        );
        assert!(schedules.iter().all(|schedule| schedule.should_report));

        let capped = retry.schedule_after_failure();
        assert_eq!(capped.delay, Duration::from_millis(5_000));
        assert!(!capped.should_report);
        assert!(retry.wait_timeout().is_some());
    }

    #[test]
    fn pending_retry_waits_until_its_deadline_but_accepts_commands() {
        let deadline = Instant::now() + Duration::from_secs(5);
        let retry = RetryBackoff {
            next_attempt: Some(deadline),
            ..Default::default()
        };
        let before = Instant::now();
        let timeout = retry.wait_timeout().unwrap();
        assert!(timeout <= deadline.saturating_duration_since(before));
        assert!(timeout >= deadline.saturating_duration_since(Instant::now()));

        let (command_tx, command_rx) = mpsc::channel();
        command_tx.send(WorkerCommand::StopPreview).unwrap();
        assert!(matches!(
            wait_for_manager_command(&command_rx, &retry),
            ManagerCommandWait::Command(WorkerCommand::StopPreview)
        ));
    }

    #[test]
    fn a_new_start_restarts_a_capped_backoff_immediately() {
        let mut retry = RetryBackoff::default();
        for _ in 0..=RETRY_DELAYS_MS.len() {
            let _ = retry.schedule_after_failure();
        }
        assert!(retry.wait_timeout().is_some());

        retry.start_now();
        assert!(retry.is_due());
        let restarted = retry.schedule_after_failure();
        assert_eq!(restarted.delay, Duration::from_millis(RETRY_DELAYS_MS[0]));
        assert!(restarted.should_report);
    }

    #[test]
    fn stop_style_cancel_removes_a_scheduled_retry() {
        let mut retry = RetryBackoff::default();
        retry.start_now();
        let _ = retry.schedule_after_failure();
        assert!(retry.wait_timeout().is_some());

        retry.cancel();
        assert_eq!(retry.wait_timeout(), None);
        assert!(!retry.is_due());
    }

    #[test]
    fn planned_restart_stops_active_session_and_notifies_ui() {
        let stop_called = Cell::new(false);
        let mut session = Some(());
        let (event_tx, event_rx) = mpsc::sync_channel(1);

        stop_session_for_planned_restart(
            &mut session,
            |_| stop_called.set(true),
            "planned-restart".to_string(),
            &event_tx,
        );

        assert!(session.is_none());
        assert!(stop_called.get());
        assert!(matches!(
            event_rx.try_recv(),
            Ok(WorkerEvent::PreviewRetrying { reason }) if reason == "planned-restart"
        ));
    }

    #[test]
    fn planned_restart_without_active_session_emits_nothing() {
        let mut session: Option<()> = None;
        let (event_tx, event_rx) = mpsc::sync_channel(1);

        stop_session_for_planned_restart(
            &mut session,
            |_| panic!("no session should be stopped"),
            "planned-restart".to_string(),
            &event_tx,
        );

        assert!(matches!(event_rx.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn disconnected_command_channel_is_terminal_with_or_without_a_timer() {
        let (command_tx, command_rx) = mpsc::channel();
        drop(command_tx);
        assert!(matches!(
            wait_for_manager_command(&command_rx, &RetryBackoff::default()),
            ManagerCommandWait::Disconnected
        ));

        let (command_tx, command_rx) = mpsc::channel();
        drop(command_tx);
        let mut retry = RetryBackoff::default();
        retry.start_now();
        let _ = retry.schedule_after_failure();
        assert!(matches!(
            wait_for_manager_command(&command_rx, &retry),
            ManagerCommandWait::Disconnected
        ));
    }

    #[test]
    fn dropping_the_last_sender_stops_the_worker() {
        let (command_tx, _event_rx, worker) =
            spawn_camera_worker_thread(CameraConfig::default());
        drop(command_tx);
        worker.join().expect("camera worker should stop");
    }

    #[test]
    fn fatal_preview_failure_clears_requested_state_and_retry() {
        let mut state = WorkerState {
            config: CameraConfig::default(),
            preview_requested: true,
        };
        let mut retry = RetryBackoff::default();
        retry.start_now();
        let (event_tx, event_rx) = mpsc::sync_channel(2);

        let result = fail_preview_permanently(
            &mut state,
            &mut retry,
            &event_tx,
            "fatal-test".to_string(),
        );

        assert!(matches!(result, PreviewAttemptResult::FatalFailure));
        assert!(!state.preview_requested);
        assert_eq!(retry.wait_timeout(), None);
        assert!(matches!(
            event_rx.try_recv(),
            Ok(WorkerEvent::PreviewStopped { reason }) if reason.contains("fatal-test")
        ));
    }

    #[test]
    fn transient_failures_recreate_only_at_the_configured_threshold() {
        let mut failures = 0;
        for _ in 1..MANAGER_RECREATE_AFTER_FAILURES {
            assert!(!record_transient_failure(&mut failures));
        }
        assert!(record_transient_failure(&mut failures));
    }

    #[test]
    fn frame_watchdog_uses_a_deterministic_boundary() {
        assert!(!frame_stalled(
            FRAME_STALL_TIMEOUT.saturating_sub(Duration::from_millis(1))
        ));
        assert!(frame_stalled(FRAME_STALL_TIMEOUT));
        assert!(frame_stalled(FRAME_STALL_TIMEOUT + Duration::from_secs(1)));
    }
}
