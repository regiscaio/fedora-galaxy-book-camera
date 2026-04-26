use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use galaxybook_camera::{preview_zoom_options, trf};
use gtk::glib::{self, ControlFlow};
use gtk::prelude::*;
use galaxybook_camera::Preset;

use super::*;

impl CameraWindow {
    pub(super) fn bind_ui(
        self: &Rc<Self>,
        event_rx: Receiver<WorkerEvent>,
        singleton_rx: Receiver<()>,
    ) {
        self.window.connect_close_request({
            let app = Rc::clone(self);
            move |_| {
                app.shutdown();
                glib::Propagation::Proceed
            }
        });

        self.preview_button.connect_clicked({
            let app = Rc::clone(self);
            move |_| {
                app.cancel_countdown(None);
                if app.state.borrow().preview_active {
                    app.stop_preview();
                } else {
                    app.start_preview();
                }
            }
        });

        self.update_button.connect_clicked({
            let app = Rc::clone(self);
            move |_| {
                app.install_updates();
            }
        });

        let show_settings_action = gtk::gio::SimpleAction::new("show-settings", None);
        show_settings_action.connect_activate({
            let app = Rc::clone(self);
            move |_, _| {
                let show_sidebar = !app.split_view.shows_sidebar();
                app.split_view.set_show_sidebar(show_sidebar);
            }
        });
        self.window.add_action(&show_settings_action);

        let show_about_action = gtk::gio::SimpleAction::new("show-about", None);
        show_about_action.connect_activate({
            let app = Rc::clone(self);
            move |_, _| {
                present_about_dialog(&app.window, &app.toast_overlay);
            }
        });
        self.window.add_action(&show_about_action);

        for (button, seconds) in [
            (&self.countdown_off_button, 0_u32),
            (&self.countdown_three_button, 3_u32),
            (&self.countdown_five_button, 5_u32),
            (&self.countdown_ten_button, 10_u32),
        ] {
            button.connect_toggled({
                let app = Rc::clone(self);
                move |button| {
                    if app.syncing_ui.get() || !button.is_active() {
                        return;
                    }

                    app.set_countdown_seconds(seconds);
                    if let Some(popover) = app.countdown_button.popover() {
                        popover.popdown();
                    }
                }
            });
        }

        self.photo_mode_button.connect_toggled({
            let app = Rc::clone(self);
            move |button| {
                if !button.is_active() {
                    return;
                }
                app.cancel_countdown(None);
                app.set_zoom_selector_expanded(false);
                {
                    let mut state = app.state.borrow_mut();
                    state.capture_mode = CaptureMode::Photo;
                }
                app.refresh_capture_controls();
            }
        });

        self.video_mode_button.connect_toggled({
            let app = Rc::clone(self);
            move |button| {
                if !button.is_active() {
                    return;
                }
                app.cancel_countdown(None);
                app.set_zoom_selector_expanded(false);
                {
                    let mut state = app.state.borrow_mut();
                    state.capture_mode = CaptureMode::Video;
                }
                app.refresh_capture_controls();
            }
        });

        self.capture_button.connect_clicked({
            let app = Rc::clone(self);
            move |_| {
                app.handle_capture_action();
            }
        });

        self.zoom_button.connect_clicked({
            let app = Rc::clone(self);
            move |_| {
                app.set_zoom_selector_expanded(true);
            }
        });

        bind_switch_row(self, &self.controls.auto_apply_row, |state, active| {
            state.auto_apply = active;
        }, false);
        self.controls.show_grid_row.connect_active_notify({
            let app = Rc::clone(self);
            move |row| {
                if app.syncing_ui.get() {
                    return;
                }

                let active = row.is_active();
                {
                    let mut state = app.state.borrow_mut();
                    state.show_grid = active;
                    state.config.show_grid = active;
                }

                app.grid_overlay.set_visible(active);
                if let Err(error) = app.persist_config() {
                    app.set_status(
                        &trf("Falha ao salvar configuração: {error}", &[("error", error)]),
                        true,
                    );
                }
            }
        });
        bind_switch_row(self, &self.controls.mirror_row, |state, active| {
            state.config.mirror = active;
        }, false);
        bind_switch_row(self, &self.controls.record_audio_row, |state, active| {
            state.config.record_audio = active;
        }, false);

        bind_scale(
            self,
            &self.controls.brightness_scale,
            &self.controls.brightness_value,
            |config, value| {
                config.brightness = value;
            },
            false,
        );
        bind_scale(
            self,
            &self.controls.exposure_scale,
            &self.controls.exposure_value,
            |config, value| {
                config.exposure_value = value;
            },
            false,
        );
        bind_scale(
            self,
            &self.controls.contrast_scale,
            &self.controls.contrast_value,
            |config, value| {
                config.contrast = value;
            },
            false,
        );
        bind_scale(
            self,
            &self.controls.saturation_scale,
            &self.controls.saturation_value,
            |config, value| {
                config.saturation = value;
            },
            false,
        );
        bind_scale(
            self,
            &self.controls.hue_scale,
            &self.controls.hue_value,
            |config, value| {
                config.hue = value;
            },
            false,
        );
        bind_scale(
            self,
            &self.controls.temperature_scale,
            &self.controls.temperature_value,
            |config, value| {
                config.temperature = value;
            },
            false,
        );
        bind_scale(
            self,
            &self.controls.tint_scale,
            &self.controls.tint_value,
            |config, value| {
                config.tint = value;
            },
            false,
        );
        bind_scale(
            self,
            &self.controls.red_scale,
            &self.controls.red_value,
            |config, value| {
                config.red_gain = value;
            },
            false,
        );
        bind_scale(
            self,
            &self.controls.green_scale,
            &self.controls.green_value,
            |config, value| {
                config.green_gain = value;
            },
            false,
        );
        bind_scale(
            self,
            &self.controls.blue_scale,
            &self.controls.blue_value,
            |config, value| {
                config.blue_gain = value;
            },
            false,
        );
        bind_scale(
            self,
            &self.controls.gamma_scale,
            &self.controls.gamma_value,
            |config, value| {
                config.gamma = value;
            },
            false,
        );
        bind_scale(
            self,
            &self.controls.sharpness_scale,
            &self.controls.sharpness_value,
            |config, value| {
                config.sharpness = value;
            },
            false,
        );

        for (index, button) in self.zoom_option_buttons.iter().enumerate() {
            button.connect_toggled({
                let app = Rc::clone(self);
                move |button| {
                    if app.syncing_ui.get() || !button.is_active() {
                        return;
                    }
                    let options = preview_zoom_options();
                    let Some(option) = options.get(index).or_else(|| options.first()) else {
                        return;
                    };
                    {
                        let mut state = app.state.borrow_mut();
                        state.config.width = Some(option.width);
                        state.config.height = Some(option.height);
                    }
                    app.refresh_zoom_controls();
                    app.on_config_changed(true);
                    app.set_zoom_selector_expanded(false);
                }
            });
        }

        self.controls.preset_row.connect_selected_notify({
            let app = Rc::clone(self);
            move |row| {
                if app.syncing_ui.get() {
                    return;
                }
                let selected = row.selected() as usize;
                {
                    let mut state = app.state.borrow_mut();
                    state.preset_index = selected;
                    state.config.apply_preset(Preset::from_index(selected));
                    state.restart_pending = true;
                }
                app.sync_controls_from_state();
                app.on_config_changed(true);
            }
        });

        self.controls.audio_source_row.connect_selected_notify({
            let app = Rc::clone(self);
            move |row| {
                if app.syncing_ui.get() {
                    return;
                }
                let selected = row.selected() as usize;
                let audio_id = app
                    .state
                    .borrow()
                    .audio_sources
                    .get(selected)
                    .map(|source| source.id.clone())
                    .unwrap_or_else(|| "default".to_string());
                app.state.borrow_mut().config.audio_source = audio_id;
                app.on_config_changed(false);
            }
        });

        self.controls.apply_button.connect_clicked({
            let app = Rc::clone(self);
            move |_| {
                let restart_required = app.state.borrow().restart_pending;
                app.apply_config_safely(restart_required);
            }
        });

        self.controls.save_button.connect_clicked({
            let app = Rc::clone(self);
            move |_| {
                if let Err(error) = app.persist_config() {
                    app.set_status(
                        &trf("Falha ao salvar configuração: {error}", &[("error", error)]),
                        true,
                    );
                    return;
                }
                app.set_status(
                    &trf(
                        "Configuração salva em {config_path}.",
                        &[("config_path", app.config_path.display().to_string())],
                    ),
                    true,
                );
            }
        });

        self.controls.reset_button.connect_clicked({
            let app = Rc::clone(self);
            move |_| {
                {
                    let mut state = app.state.borrow_mut();
                    state.config = CameraConfig::default();
                    state.restart_pending = true;
                    state.preset_index = 0;
                }
                app.sync_controls_from_state();
                app.on_config_changed(true);
            }
        });

        glib::timeout_add_local(Duration::from_millis(16), {
            let app = Rc::clone(self);
            move || {
                while singleton_rx.try_recv().is_ok() {
                    app.window.present();
                    app.window.grab_focus();
                }

                loop {
                    match event_rx.try_recv() {
                        Ok(event) => app.handle_worker_event(event),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => return ControlFlow::Break,
                    }
                }

                ControlFlow::Continue
            }
        });
    }
}

fn bind_scale<F>(
    app: &Rc<CameraWindow>,
    scale: &gtk::Scale,
    value_label: &gtk::Label,
    setter: F,
    restart_required: bool,
) where
    F: Fn(&mut CameraConfig, f64) + 'static,
{
    scale.connect_value_changed({
        let app = Rc::clone(app);
        let value_label = value_label.clone();
        move |scale| {
            let value = scale.value();
            value_label.set_label(&format!("{value:.2}"));
            if app.syncing_ui.get() {
                return;
            }

            {
                let mut state = app.state.borrow_mut();
                setter(&mut state.config, value);
            }
            app.on_config_changed(restart_required);
        }
    });
}

fn bind_switch_row<F>(
    app: &Rc<CameraWindow>,
    row: &adw::SwitchRow,
    setter: F,
    restart_required: bool,
) where
    F: Fn(&mut WindowState, bool) + 'static,
{
    row.connect_active_notify({
        let app = Rc::clone(app);
        move |row| {
            if app.syncing_ui.get() {
                return;
            }

            {
                let mut state = app.state.borrow_mut();
                setter(&mut state, row.is_active());
            }
            app.on_config_changed(restart_required);
        }
    });
}
