use std::fs;
use std::rc::Rc;

use galaxybook_camera::{photo_library_dir, timestamp};
use gtk::glib::{self, ControlFlow};

use super::*;

impl CameraWindow {
    pub(super) fn stop_preview(&self) {
        self.cancel_countdown(None);
        let _ = self.command_tx.send(WorkerCommand::StopPreview);
        self.set_status("Parando preview...", false);
    }

    pub(super) fn capture_photo_now(&self) {
        let output_dir = photo_library_dir();
        if let Err(error) = fs::create_dir_all(&output_dir) {
            self.set_status(
                &format!("Falha ao preparar a pasta da câmera: {error}"),
                true,
            );
            return;
        }

        let output_path = output_dir.join(format!("camera-{}.jpg", timestamp()));
        let _ = self.command_tx.send(WorkerCommand::CapturePhoto {
            output_path: output_path.clone(),
        });
        self.set_status(
            &format!(
                "Capturando foto em resolução máxima em {}...",
                output_path.display()
            ),
            false,
        );
    }

    pub(super) fn start_recording_now(&self) {
        let preview_active = self.state.borrow().preview_active;
        if !preview_active {
            self.set_status("Inicie o preview antes de gravar vídeo.", true);
            return;
        }

        let _ = self.command_tx.send(WorkerCommand::StartRecording);
        self.state.borrow_mut().is_recording = true;
        self.refresh_capture_controls();
        self.set_status("Aguardando o próximo frame para iniciar a gravação...", false);
    }

    pub(super) fn stop_recording_now(&self) {
        if !self.state.borrow().is_recording {
            return;
        }

        let _ = self.command_tx.send(WorkerCommand::StopRecording);
        self.state.borrow_mut().is_recording = false;
        self.refresh_capture_controls();
        self.set_status("Finalizando arquivo de vídeo...", false);
    }

    pub(super) fn handle_capture_action(self: &Rc<Self>) {
        if self.state.borrow().countdown_remaining.is_some() {
            self.cancel_countdown(Some("Contagem regressiva cancelada."));
            return;
        }

        let (capture_mode, is_recording, preview_active, countdown_seconds) = {
            let state = self.state.borrow();
            (
                state.capture_mode,
                state.is_recording,
                state.preview_active,
                normalize_countdown_seconds(state.config.countdown_seconds),
            )
        };

        match capture_mode {
            CaptureMode::Photo if countdown_seconds > 0 => {
                self.start_countdown(PendingCaptureAction::Photo, countdown_seconds);
            }
            CaptureMode::Photo => self.capture_photo_now(),
            CaptureMode::Video if is_recording => self.stop_recording_now(),
            CaptureMode::Video if !preview_active => {
                self.set_status("Inicie o preview antes de gravar vídeo.", true);
            }
            CaptureMode::Video if countdown_seconds > 0 => {
                self.start_countdown(PendingCaptureAction::StartRecording, countdown_seconds);
            }
            CaptureMode::Video => self.start_recording_now(),
        }
    }

    pub(super) fn start_countdown(self: &Rc<Self>, action: PendingCaptureAction, seconds: u32) {
        self.cancel_countdown(None);

        {
            let mut state = self.state.borrow_mut();
            state.countdown_remaining = Some(seconds);
            state.pending_capture_action = Some(action);
        }

        self.refresh_preview_chrome();
        self.refresh_countdown_controls();
        self.refresh_capture_controls();
        self.set_status(&countdown_status_message(action, seconds), false);

        let source_id = glib::timeout_add_seconds_local(1, {
            let app = Rc::clone(self);
            move || app.on_countdown_tick()
        });
        *self.countdown_source.borrow_mut() = Some(source_id);
    }

    pub(super) fn on_countdown_tick(&self) -> ControlFlow {
        let action_to_execute = {
            let mut state = self.state.borrow_mut();
            match state.countdown_remaining {
                Some(remaining) if remaining > 1 => {
                    let next_value = remaining - 1;
                    state.countdown_remaining = Some(next_value);
                    None
                }
                Some(_) => {
                    state.countdown_remaining = None;
                    state.pending_capture_action.take()
                }
                None => return ControlFlow::Break,
            }
        };

        self.refresh_preview_chrome();
        self.refresh_countdown_controls();
        self.refresh_capture_controls();

        if let Some(action) = action_to_execute {
            let _ = self.countdown_source.borrow_mut().take();
            match action {
                PendingCaptureAction::Photo => self.capture_photo_now(),
                PendingCaptureAction::StartRecording => self.start_recording_now(),
            }
            ControlFlow::Break
        } else {
            let (remaining, action) = {
                let state = self.state.borrow();
                (state.countdown_remaining, state.pending_capture_action)
            };
            if let (Some(remaining), Some(action)) = (remaining, action) {
                self.set_status(&countdown_status_message(action, remaining), false);
            }
            ControlFlow::Continue
        }
    }

    pub(super) fn cancel_countdown(&self, message: Option<&str>) {
        let was_active = self.state.borrow().countdown_remaining.is_some();
        if !was_active {
            return;
        }

        if let Some(source_id) = self.countdown_source.borrow_mut().take() {
            source_id.remove();
        }

        {
            let mut state = self.state.borrow_mut();
            state.countdown_remaining = None;
            state.pending_capture_action = None;
        }

        self.refresh_preview_chrome();
        self.refresh_countdown_controls();
        self.refresh_capture_controls();

        if let Some(message) = message {
            self.set_status(message, false);
        }
    }

    pub(super) fn set_countdown_seconds(&self, seconds: u32) {
        self.cancel_countdown(None);
        {
            let mut state = self.state.borrow_mut();
            state.config.countdown_seconds = normalize_countdown_seconds(seconds);
        }

        if let Err(error) = self.persist_config() {
            self.set_status(&format!("Falha ao salvar configuracao: {error}"), true);
            return;
        }

        self.syncing_ui.set(true);
        self.countdown_off_button.set_active(seconds == 0);
        self.countdown_three_button.set_active(seconds == 3);
        self.countdown_ten_button.set_active(seconds == 10);
        self.syncing_ui.set(false);
        self.refresh_countdown_controls();
    }
}

fn countdown_status_message(action: PendingCaptureAction, seconds: u32) -> String {
    match action {
        PendingCaptureAction::Photo => format!("Foto em {seconds}s..."),
        PendingCaptureAction::StartRecording => format!("Vídeo em {seconds}s..."),
    }
}
