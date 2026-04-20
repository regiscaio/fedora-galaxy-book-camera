use gtk::gdk;
use gtk::glib;

use super::*;

impl CameraWindow {
    pub(super) fn handle_worker_event(&self, event: WorkerEvent) {
        match event {
            WorkerEvent::PreviewStarted { width, height } => {
                let mut state = self.state.borrow_mut();
                state.preview_active = true;
                state.preview_size = Some((width, height));
                state.status = format!("Preview ativo em {}x{}.", width, height);
                drop(state);
                self.status_label.set_label("Preview ativo.");
                self.refresh_preview_chrome();
                self.refresh_header_metrics();
            }
            WorkerEvent::PreviewStopped { reason } => {
                let mut state = self.state.borrow_mut();
                state.preview_active = false;
                state.is_recording = false;
                state.fps = 0.0;
                state.status = state.post_stop_status.take().unwrap_or(reason);
                drop(state);
                self.picture.set_paintable(Option::<&gdk::Paintable>::None);
                self.status_label.set_label(&self.state.borrow().status);
                self.refresh_preview_chrome();
                self.refresh_capture_controls();
                self.refresh_header_metrics();
            }
            WorkerEvent::PreviewFrame { frame, fps } => {
                self.present_frame(frame);
                {
                    let mut state = self.state.borrow_mut();
                    state.fps = fps;
                }
                self.refresh_header_metrics();
            }
            WorkerEvent::Status(message) => {
                self.set_status(&message, false);
            }
            WorkerEvent::PhotoFinished {
                success,
                output_path,
                stderr,
                resolution,
            } => {
                if success {
                    self.state.borrow_mut().last_media_path = Some(output_path.clone());
                    if let Some((width, height)) = resolution {
                        self.set_status(
                            &format!(
                                "Foto máxima salva em {} ({}x{}).",
                                output_path.display(),
                                width,
                                height
                            ),
                            true,
                        );
                    } else {
                        self.set_status(
                            &format!("Foto salva em {}.", output_path.display()),
                            true,
                        );
                    }
                } else if stderr.is_empty() {
                    self.set_status("Falha ao salvar foto.", true);
                } else {
                    self.set_status(&format!("Falha ao salvar foto: {stderr}"), true);
                }
            }
            WorkerEvent::RecordingFinished {
                success,
                output_path,
                stderr,
            } => {
                self.state.borrow_mut().is_recording = false;
                self.refresh_capture_controls();
                if success {
                    self.state.borrow_mut().last_media_path = Some(output_path.clone());
                    self.set_status(
                        &format!("Vídeo salvo em {}.", output_path.display()),
                        true,
                    );
                } else if stderr.is_empty() {
                    self.set_status("Falha ao gravar vídeo.", true);
                } else {
                    self.set_status(&format!("Falha ao gravar vídeo: {stderr}"), true);
                }
            }
        }
    }

    fn present_frame(&self, frame: OwnedFrame) {
        let stride = frame.width * 4;
        let bytes = glib::Bytes::from_owned(frame.data);
        let texture = gdk::MemoryTexture::new(
            frame.width as i32,
            frame.height as i32,
            gdk::MemoryFormat::R8g8b8a8,
            &bytes,
            stride,
        );
        self.picture.set_paintable(Some(&texture));
        {
            let mut state = self.state.borrow_mut();
            state.preview_size = Some((frame.width, frame.height));
        }
        self.placeholder.set_visible(false);
    }
}
