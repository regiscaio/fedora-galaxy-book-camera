use std::fs;

use galaxybook_camera::{preview_zoom_options, tr, trf};

use super::*;

impl CameraWindow {
    pub fn start_preview(&self) {
        self.cancel_countdown(None);
        if let Err(error) = self.persist_config() {
            self.set_status(
                &trf("Falha ao salvar configuração: {error}", &[("error", error)]),
                true,
            );
            return;
        }

        {
            let mut state = self.state.borrow_mut();
            state.preview_active = true;
            state.post_stop_status = None;
        }
        let _ = self.command_tx.send(WorkerCommand::StartPreview);
        self.set_status(&tr("Iniciando preview..."), false);
        self.refresh_preview_chrome();
        self.refresh_header_metrics();
    }

    pub(super) fn persist_config(&self) -> Result<(), String> {
        self.state.borrow().config.save(&self.config_path)
    }

    pub(super) fn on_config_changed(&self, restart_required: bool) {
        self.cancel_countdown(None);
        {
            let mut state = self.state.borrow_mut();
            if restart_required {
                state.restart_pending = true;
            }
        }

        if self.state.borrow().auto_apply {
            self.apply_config_safely(restart_required);
        } else {
            if let Err(error) = self.persist_config() {
                self.set_status(
                    &trf("Falha ao salvar configuração: {error}", &[("error", error)]),
                    true,
                );
                return;
            }

            if restart_required {
                self.set_status(
                    &tr("Configuração salva. A nova resolução ou preset será aplicada no próximo preview."),
                    false,
                );
            } else {
                self.set_status(
                    &tr("Configuração salva. Clique em Aplicar para enviar ao preview."),
                    false,
                );
            }
        }
    }

    pub(super) fn apply_config_safely(&self, restart_required: bool) {
        if let Err(error) = self.persist_config() {
            self.set_status(
                &trf("Falha ao salvar configuração: {error}", &[("error", error)]),
                true,
            );
            return;
        }

        let config = self.state.borrow().config.clone();
        let preview_active = self.state.borrow().preview_active;

        if restart_required {
            self.state.borrow_mut().restart_pending = false;
            let _ = self.command_tx.send(WorkerCommand::ApplyConfig {
                config,
                restart: preview_active,
            });

            if preview_active {
                self.set_status(
                    &tr("Reiniciando o preview para aplicar a nova resolução ou preset..."),
                    false,
                );
            } else {
                self.set_status(
                    &tr("Nova resolução ou preset salvo. O próximo preview já abrirá com a nova configuração."),
                    false,
                );
            }
        } else {
            let _ = self.command_tx.send(WorkerCommand::ApplyConfig {
                config,
                restart: false,
            });
            self.set_status(&tr("Ajustes aplicados."), false);
        }
    }

    pub(super) fn shutdown(&self) {
        if self.shutdown_sent.replace(true) {
            return;
        }
        self.cancel_countdown(None);
        let _ = self.command_tx.send(WorkerCommand::Shutdown);
        let _ = self.persist_config();
        let _ = fs::remove_file(&self.singleton_socket_path);
    }
}

pub(super) fn apply_validated_startup_resolution(config: &mut CameraConfig) {
    let options = preview_zoom_options();
    let Some(option) = options
        .iter()
        .find(|option| Some(option.width) == config.width && Some(option.height) == config.height)
        .or_else(|| options.first())
    else {
        return;
    };

    config.width = Some(option.width);
    config.height = Some(option.height);
}
