use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{
    self,
    Receiver,
    RecvTimeoutError,
    Sender,
    SyncSender,
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
    request::{Request, ReuseFlag},
    stream::{Stream, StreamRole},
};

use crate::{
    apply_adjustments,
    capture_photo_max_resolution_with_manager,
    preview_dimensions,
    set_softisp_env,
    spawn_video_recorder,
    timestamp,
    tr,
    trf,
    video_library_dir,
    AdjustmentProfile,
    CameraConfig,
    OwnedFrame,
    RecorderHandle,
};

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
    last_fps: f32,
    fps_window_started: Instant,
    fps_window_frames: u32,
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

pub fn spawn_camera_worker(
    initial_config: CameraConfig,
) -> (Sender<WorkerCommand>, Receiver<WorkerEvent>) {
    let (command_tx, command_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(2);
    thread::spawn(move || camera_worker(initial_config, command_rx, event_tx));
    (command_tx, event_rx)
}

fn camera_worker(
    initial_config: CameraConfig,
    command_rx: Receiver<WorkerCommand>,
    event_tx: SyncSender<WorkerEvent>,
) {
    let mut config = initial_config;
    set_softisp_env(&config.softisp_mode);

    let manager = match CameraManager::new() {
        Ok(manager) => manager,
        Err(error) => {
            let _ = event_tx.send(WorkerEvent::PreviewStopped {
                reason: trf("Falha ao iniciar o libcamera: {error}", &[("error", error.to_string())]),
            });
            return;
        }
    };

    let mut session = None;

    loop {
        while let Ok(command) = command_rx.try_recv() {
            if handle_worker_command(command, &manager, &mut config, &mut session, &event_tx) {
                if let Some(mut session) = session.take() {
                    stop_preview_session(&mut session);
                }
                return;
            }
        }

        let Some(active_session) = session.as_mut() else {
            match command_rx.recv() {
                Ok(command) => {
                    if handle_worker_command(
                        command,
                        &manager,
                        &mut config,
                        &mut session,
                        &event_tx,
                    ) {
                        return;
                    }
                }
                Err(_) => return,
            }
            continue;
        };

        match active_session.request_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(mut request) => {
                if let Err(error) =
                    process_completed_request(active_session, &config, &mut request, &event_tx)
                {
                    let _ = event_tx.send(WorkerEvent::Status(trf(
                        "Erro no preview: {error}",
                        &[("error", error)],
                    )));
                }

                request.reuse(ReuseFlag::REUSE_BUFFERS);
                if let Err((_, error)) = active_session.camera.queue_request(request) {
                    let _ = event_tx.send(WorkerEvent::PreviewStopped {
                        reason: trf(
                            "Falha ao reenfileirar o frame da câmera: {error}",
                            &[("error", error.to_string())],
                        ),
                    });
                    let mut old_session = session.take().unwrap();
                    stop_preview_session(&mut old_session);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                let _ = event_tx.send(WorkerEvent::PreviewStopped {
                    reason: tr("Canal interno do preview foi encerrado."),
                });
                if let Some(mut old_session) = session.take() {
                    stop_preview_session(&mut old_session);
                }
            }
        }
    }
}

fn handle_worker_command<'a>(
    command: WorkerCommand,
    manager: &'a CameraManager,
    config: &mut CameraConfig,
    session: &mut Option<PreviewSession<'a>>,
    event_tx: &SyncSender<WorkerEvent>,
) -> bool {
    match command {
        WorkerCommand::StartPreview => {
            *session = restart_preview_session(manager, config, session.take(), event_tx);
            false
        }
        WorkerCommand::StopPreview => {
            if let Some(mut active_session) = session.take() {
                stop_preview_session(&mut active_session);
            }
            let _ = event_tx.send(WorkerEvent::PreviewStopped {
                reason: tr("Preview parado."),
            });
            false
        }
        WorkerCommand::ApplyConfig {
            config: next_config,
            restart,
        } => {
            *config = next_config;
            if let Some(active_session) = session.as_mut() {
                active_session.update_profile(config);
            }
            if restart && session.is_some() {
                *session = restart_preview_session(manager, config, session.take(), event_tx);
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

            let should_restore_preview = session.is_some();
            if let Some(mut active_session) = session.take() {
                stop_preview_session(&mut active_session);
            }

            let result = capture_photo_max_resolution_with_manager(manager, config, &output_path);

            if should_restore_preview {
                *session = restart_preview_session(manager, config, session.take(), event_tx);
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
                let _ = event_tx.send(WorkerEvent::Status(
                    tr("Inicie o preview antes de gravar vídeo."),
                ));
                return false;
            };
            if active_session.recorder.is_some()
                || active_session.pending_recording_output.is_some()
            {
                return false;
            }
            let output_path = video_library_dir().join(format!("camera-{}.mp4", timestamp()));
            active_session.pending_recording_output = Some(output_path.clone());
            let _ = event_tx.send(WorkerEvent::Status(trf(
                "Preparando gravação em {output_path}...",
                &[("output_path", output_path.display().to_string())],
            )));
            false
        }
        WorkerCommand::StopRecording => {
            let Some(active_session) = session.as_mut() else {
                return false;
            };
            active_session.pending_recording_output = None;
            if let Some(recorder) = active_session.recorder.take() {
                drop(recorder);
                let _ = event_tx.send(WorkerEvent::Status(
                    tr("Finalizando arquivo de vídeo..."),
                ));
            }
            false
        }
        WorkerCommand::Shutdown => true,
    }
}

fn restart_preview_session<'a>(
    manager: &'a CameraManager,
    config: &CameraConfig,
    previous_session: Option<PreviewSession<'a>>,
    event_tx: &SyncSender<WorkerEvent>,
) -> Option<PreviewSession<'a>> {
    if let Some(mut session) = previous_session {
        stop_preview_session(&mut session);
    }

    match start_preview_session(manager, config) {
        Ok(session) => {
            let _ = event_tx.send(WorkerEvent::PreviewStarted {
                width: session.width,
                height: session.height,
            });
            let _ = event_tx.send(WorkerEvent::Status(trf(
                "Preview ativo em {width}x{height} com libcamera direto.",
                &[
                    ("width", session.width.to_string()),
                    ("height", session.height.to_string()),
                ],
            )));
            Some(session)
        }
        Err(error) => {
            let _ = event_tx.send(WorkerEvent::PreviewStopped {
                reason: error.clone(),
            });
            None
        }
    }
}

fn start_preview_session<'a>(
    manager: &'a CameraManager,
    config: &CameraConfig,
) -> Result<PreviewSession<'a>, String> {
    let camera_id = manager
        .cameras()
        .iter()
        .next()
        .map(|camera| camera.id().to_string())
        .ok_or_else(|| tr("Nenhuma câmera disponível no libcamera."))?;
    let camera_ref = manager
        .get(&camera_id)
        .ok_or_else(|| trf("Câmera {camera_id} não ficou acessível pelo CameraManager.", &[("camera_id", camera_id.clone())]))?;
    let mut camera = camera_ref
        .acquire()
        .map_err(|error| trf("Falha ao adquirir a câmera: {error}", &[("error", error.to_string())]))?;

    let mut configuration = camera
        .generate_configuration(&[StreamRole::ViewFinder])
        .ok_or_else(|| tr("Não foi possível gerar a configuração padrão da câmera."))?;
    let Some(mut stream_cfg) = configuration.get_mut(0) else {
        return Err(tr("A configuração da câmera não retornou um stream válido."));
    };

    let pixel_format = PixelFormat::parse("XBGR8888")
        .ok_or_else(|| tr("XBGR8888 não está disponível neste host."))?;
    stream_cfg.set_pixel_format(pixel_format);
    if let (Some(width), Some(height)) = (config.width, config.height) {
        stream_cfg.set_size(Size::new(width, height));
    }

    match configuration.validate() {
        CameraConfigurationStatus::Invalid => {
            return Err(tr("A configuração solicitada ficou inválida depois da validação."))
        }
        CameraConfigurationStatus::Adjusted | CameraConfigurationStatus::Valid => {}
    }

    camera
        .configure(&mut configuration)
        .map_err(|error| trf("Falha ao configurar a câmera: {error}", &[("error", error.to_string())]))?;

    let stream_cfg = configuration
        .get(0)
        .ok_or_else(|| tr("Não foi possível ler o stream configurado."))?;
    let stream = stream_cfg.stream().ok_or_else(|| {
        tr("O stream configurado não ficou disponível depois do configure().")
    })?;
    let size = stream_cfg.get_size();
    let width = size.width as usize;
    let height = size.height as usize;
    let stride = stream_cfg.get_stride() as usize;

    let mut allocator = FrameBufferAllocator::new(&camera);
    let buffers = allocator
        .alloc(&stream)
        .map_err(|error| trf("Falha ao alocar buffers da câmera: {error}", &[("error", error.to_string())]))?;
    let buffers = buffers
        .into_iter()
        .map(|buffer| {
            MemoryMappedFrameBuffer::new(buffer)
                .map_err(|error| trf("Falha ao mapear buffer da câmera na memória: {error}", &[("error", error.to_string())]))
        })
        .collect::<Result<Vec<MemoryMappedFrameBuffer<CameraFrameBuffer>>, String>>()?;

    let mut requests = Vec::with_capacity(buffers.len());
    for (index, buffer) in buffers.into_iter().enumerate() {
        let mut request = camera
            .create_request(Some(index as u64))
            .ok_or_else(|| tr("Falha ao criar uma requisição de captura."))?;
        request
            .add_buffer(&stream, buffer)
            .map_err(|error| trf("Falha ao anexar buffer à requisição: {error}", &[("error", error.to_string())]))?;
        requests.push(request);
    }

    let request_rx = camera.subscribe_request_completed();
    camera
        .start(None)
        .map_err(|error| trf("Falha ao iniciar a captura: {error}", &[("error", error.to_string())]))?;

    for request in requests {
        camera
            .queue_request(request)
            .map_err(|(_, error)| trf("Falha ao enfileirar requisição inicial: {error}", &[("error", error.to_string())]))?;
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
        last_fps: 0.0,
        fps_window_started: Instant::now(),
        fps_window_frames: 0,
    })
}

fn stop_preview_session(session: &mut PreviewSession<'_>) {
    session.pending_recording_output = None;
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
                    recorder.try_send_frame(&output_frame);
                    let backend_label = recorder.backend.ui_label();
                    session.recorder = Some(recorder);
                    let _ = event_tx.send(WorkerEvent::Status(trf(
                        "Gravando vídeo em {output_path} usando {backend_label}.",
                        &[
                            ("output_path", output_path.display().to_string()),
                            ("backend_label", backend_label),
                        ],
                    )));
                }
                Err(error) => {
                    let _ = event_tx.send(WorkerEvent::Status(error));
                }
            }
        }

        if let Some(recorder) = session.recorder.as_ref() {
            recorder.try_send_frame(&output_frame);
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
