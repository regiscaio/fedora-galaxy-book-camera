use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use galaxybook_camera::{
    default_config_path, detect_audio_sources, localized_app_name,
    normalize_countdown_seconds, photo_library_dir, preferred_video_encoder_backend,
    preview_zoom_options, run_smoke_test, set_softisp_env, setup_singleton,
    spawn_camera_worker, timestamp, APP_ID, AudioSourceOption, CameraConfig,
    CaptureMode, OwnedFrame, Preset, SingletonState, WorkerCommand, WorkerEvent,
};
use adw::prelude::*;
use gtk::gdk;
use gtk::glib::{self, ControlFlow};
use gtk::prelude::*;
use gtk::{Align, Orientation};
use libadwaita as adw;

mod ui;

use ui::{
    apply_application_css,
    build_about_details_subpage,
    build_about_summary_row,
    build_control_widgets,
    build_sidebar,
    build_suffix_action_row,
    build_zoom_selector,
    draw_preview_grid,
    selected_audio_index,
    set_scale_value,
    ControlWidgets,
};

const WINDOW_WIDTH: i32 = 1320;
const WINDOW_HEIGHT: i32 = 880;

struct CliArgs {
    smoke_test: bool,
    capture_photo_once: Option<PathBuf>,
    config_path: PathBuf,
}

struct WindowState {
    config: CameraConfig,
    auto_apply: bool,
    preset_index: usize,
    capture_mode: CaptureMode,
    preview_active: bool,
    is_recording: bool,
    show_grid: bool,
    countdown_remaining: Option<u32>,
    fps: f32,
    preview_size: Option<(usize, usize)>,
    restart_pending: bool,
    post_stop_status: Option<String>,
    last_media_path: Option<PathBuf>,
    status: String,
    audio_sources: Vec<AudioSourceOption>,
    pending_capture_action: Option<PendingCaptureAction>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingCaptureAction {
    Photo,
    StartRecording,
}

struct CameraWindow {
    window: adw::ApplicationWindow,
    toast_overlay: adw::ToastOverlay,
    split_view: adw::OverlaySplitView,
    title_widget: adw::WindowTitle,
    picture: gtk::Picture,
    placeholder: adw::StatusPage,
    countdown_overlay_label: gtk::Label,
    grid_overlay: gtk::DrawingArea,
    preview_button: gtk::Button,
    countdown_button: gtk::MenuButton,
    countdown_off_button: gtk::CheckButton,
    countdown_three_button: gtk::CheckButton,
    countdown_ten_button: gtk::CheckButton,
    zoom_button: gtk::Button,
    zoom_strip: gtk::Box,
    zoom_label: gtk::Label,
    zoom_option_buttons: Vec<gtk::ToggleButton>,
    capture_button: gtk::Button,
    capture_button_glyph: gtk::Box,
    photo_mode_button: gtk::ToggleButton,
    video_mode_button: gtk::ToggleButton,
    status_label: gtk::Label,
    controls: ControlWidgets,
    state: RefCell<WindowState>,
    syncing_ui: Cell<bool>,
    shutdown_sent: Cell<bool>,
    countdown_source: RefCell<Option<glib::SourceId>>,
    config_path: PathBuf,
    singleton_socket_path: PathBuf,
    command_tx: Sender<WorkerCommand>,
}

impl CameraWindow {
    fn new(
        app: &adw::Application,
        config_path: PathBuf,
        config: CameraConfig,
        singleton: SingletonState,
    ) -> Rc<Self> {
        apply_application_css();
        let app_name = localized_app_name();

        let audio_sources = detect_audio_sources();
        let (command_tx, event_rx) = spawn_camera_worker(config.clone());

        let title_widget = adw::WindowTitle::builder()
            .title(app_name.as_str())
            .build();

        let preview_button = gtk::Button::builder()
            .icon_name("media-playback-start-symbolic")
            .tooltip_text("Iniciar preview")
            .build();
        preview_button.add_css_class("flat");

        let settings_menu = gtk::gio::Menu::new();
        settings_menu.append(Some("Preferências"), Some("win.show-settings"));
        settings_menu.append(Some("Sobre"), Some("win.show-about"));

        let settings_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Abrir menu")
            .menu_model(&settings_menu)
            .build();
        settings_button.add_css_class("flat");

        let countdown_box = gtk::Box::new(Orientation::Vertical, 6);
        countdown_box.set_margin_top(12);
        countdown_box.set_margin_bottom(12);
        countdown_box.set_margin_start(12);
        countdown_box.set_margin_end(12);

        let countdown_title = gtk::Label::new(Some("Contagem regressiva"));
        countdown_title.add_css_class("heading");
        countdown_title.set_xalign(0.0);
        countdown_box.append(&countdown_title);

        let countdown_off_button = gtk::CheckButton::with_label("Desligado");
        countdown_off_button.set_active(true);
        countdown_box.append(&countdown_off_button);

        let countdown_three_button = gtk::CheckButton::with_label("3s");
        countdown_three_button.set_group(Some(&countdown_off_button));
        countdown_box.append(&countdown_three_button);

        let countdown_ten_button = gtk::CheckButton::with_label("10s");
        countdown_ten_button.set_group(Some(&countdown_off_button));
        countdown_box.append(&countdown_ten_button);

        let countdown_popover = gtk::Popover::builder().child(&countdown_box).build();
        let countdown_button = gtk::MenuButton::builder()
            .icon_name("camera-timer-symbolic")
            .tooltip_text("Contagem regressiva")
            .popover(&countdown_popover)
            .build();
        countdown_button.add_css_class("flat");

        let header_bar = adw::HeaderBar::new();
        header_bar.pack_start(&preview_button);
        header_bar.set_title_widget(Some(&title_widget));
        header_bar.pack_end(&settings_button);
        header_bar.pack_end(&countdown_button);

        let picture = gtk::Picture::new();
        picture.set_hexpand(true);
        picture.set_vexpand(true);
        picture.set_can_shrink(true);
        picture.set_content_fit(gtk::ContentFit::Cover);
        picture.add_css_class("camera-preview");

        let placeholder = adw::StatusPage::builder()
            .icon_name("camera-photo-symbolic")
            .title("Preview parado")
            .description("Clique em Iniciar preview para ativar a câmera.")
            .build();
        placeholder.add_css_class("camera-placeholder");
        placeholder.set_halign(Align::Center);
        placeholder.set_valign(Align::Center);
        placeholder.set_size_request(420, -1);

        let grid_overlay = gtk::DrawingArea::new();
        grid_overlay.set_hexpand(true);
        grid_overlay.set_vexpand(true);
        grid_overlay.set_halign(Align::Fill);
        grid_overlay.set_valign(Align::Fill);
        grid_overlay.set_draw_func(draw_preview_grid);

        let countdown_overlay_label = gtk::Label::new(None);
        countdown_overlay_label.set_halign(Align::Center);
        countdown_overlay_label.set_valign(Align::Center);
        countdown_overlay_label.set_visible(false);
        countdown_overlay_label.add_css_class("camera-countdown-overlay");

        let preview_overlay = gtk::Overlay::new();
        preview_overlay.set_hexpand(true);
        preview_overlay.set_vexpand(true);
        preview_overlay.add_css_class("camera-stage");
        preview_overlay.set_child(Some(&picture));
        preview_overlay.add_overlay(&grid_overlay);
        preview_overlay.add_overlay(&countdown_overlay_label);
        preview_overlay.add_overlay(&placeholder);

        let status_label = gtk::Label::new(Some("Preview parado. Clique em Iniciar preview."));
        status_label.set_xalign(0.0);
        status_label.set_wrap(true);
        status_label.add_css_class("dim-label");

        let photo_mode_button = gtk::ToggleButton::builder()
            .icon_name("camera-photo-symbolic")
            .tooltip_text("Modo foto")
            .build();
        photo_mode_button.add_css_class("flat");
        photo_mode_button.add_css_class("camera-mode-button");
        photo_mode_button.set_active(true);

        let video_mode_button = gtk::ToggleButton::builder()
            .icon_name("camera-video-symbolic")
            .tooltip_text("Modo vídeo")
            .build();
        video_mode_button.add_css_class("flat");
        video_mode_button.add_css_class("camera-mode-button");
        video_mode_button.set_group(Some(&photo_mode_button));

        let (zoom_root, zoom_button, zoom_strip, zoom_label, zoom_option_buttons) =
            build_zoom_selector();

        let capture_button_glyph = gtk::Box::new(Orientation::Vertical, 0);
        capture_button_glyph.add_css_class("capture-button-glyph");
        capture_button_glyph.set_halign(Align::Center);
        capture_button_glyph.set_valign(Align::Center);

        let capture_button = gtk::Button::new();
        capture_button.set_tooltip_text(Some("Tirar foto"));
        capture_button.set_child(Some(&capture_button_glyph));
        capture_button.add_css_class("capture-button");
        capture_button.set_size_request(72, 72);
        capture_button.set_halign(Align::Center);
        capture_button.set_valign(Align::Center);

        let mode_box = gtk::Box::new(Orientation::Horizontal, 8);
        mode_box.add_css_class("camera-mode-box");
        mode_box.append(&photo_mode_button);
        mode_box.append(&video_mode_button);
        mode_box.append(&zoom_root);

        let hud_box = gtk::Box::new(Orientation::Vertical, 14);
        hud_box.add_css_class("camera-hud");
        hud_box.set_halign(Align::Center);
        hud_box.set_valign(Align::End);
        hud_box.set_margin_bottom(26);
        hud_box.append(&mode_box);
        hud_box.append(&capture_button);
        preview_overlay.add_overlay(&hud_box);

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header_bar);
        toolbar_view.set_content(Some(&preview_overlay));

        let controls = build_control_widgets(&audio_sources);
        let sidebar = build_sidebar(&controls, preferred_video_encoder_backend());

        let split_view = adw::OverlaySplitView::new();
        split_view.set_sidebar_position(gtk::PackType::End);
        split_view.set_collapsed(true);
        split_view.set_enable_show_gesture(true);
        split_view.set_enable_hide_gesture(true);
        split_view.set_show_sidebar(false);
        split_view.set_content(Some(&toolbar_view));
        split_view.set_sidebar(Some(&sidebar));

        let toast_overlay = adw::ToastOverlay::new();
        toast_overlay.set_child(Some(&split_view));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(app_name.as_str())
            .default_width(WINDOW_WIDTH)
            .default_height(WINDOW_HEIGHT)
            .content(&toast_overlay)
            .build();

        let show_grid = config.show_grid;
        let state = WindowState {
            config,
            auto_apply: true,
            preset_index: 0,
            capture_mode: CaptureMode::Photo,
            preview_active: false,
            is_recording: false,
            show_grid,
            countdown_remaining: None,
            fps: 0.0,
            preview_size: None,
            restart_pending: false,
            post_stop_status: None,
            last_media_path: None,
            status: "Preview parado. Clique em Iniciar preview.".to_string(),
            audio_sources,
            pending_capture_action: None,
        };

        let app = Rc::new(Self {
            window,
            toast_overlay,
            split_view,
            title_widget,
            picture,
            placeholder,
            countdown_overlay_label,
            grid_overlay,
            preview_button,
            countdown_button,
            countdown_off_button,
            countdown_three_button,
            countdown_ten_button,
            zoom_button,
            zoom_strip,
            zoom_label,
            zoom_option_buttons,
            capture_button,
            capture_button_glyph,
            photo_mode_button,
            video_mode_button,
            status_label,
            controls,
            state: RefCell::new(state),
            syncing_ui: Cell::new(false),
            shutdown_sent: Cell::new(false),
            countdown_source: RefCell::new(None),
            config_path,
            singleton_socket_path: singleton.socket_path,
            command_tx,
        });

        app.sync_controls_from_state();
        app.refresh_header_metrics();
        app.refresh_preview_chrome();
        app.refresh_countdown_controls();
        app.refresh_capture_controls();
        app.bind_ui(event_rx, singleton.signal_rx);

        app
    }

    fn bind_ui(self: &Rc<Self>, event_rx: Receiver<WorkerEvent>, singleton_rx: Receiver<()>) {
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
                app.present_about_dialog();
            }
        });
        self.window.add_action(&show_about_action);

        for (button, seconds) in [
            (&self.countdown_off_button, 0_u32),
            (&self.countdown_three_button, 3_u32),
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
                    app.set_status(&format!("Falha ao salvar configuracao: {error}"), true);
                }
            }
        });
        bind_switch_row(self, &self.controls.mirror_row, |state, active| {
            state.config.mirror = active;
        }, false);
        bind_switch_row(self, &self.controls.record_audio_row, |state, active| {
            state.config.record_audio = active;
        }, false);

        bind_scale(self, &self.controls.brightness_scale, &self.controls.brightness_value, |config, value| {
            config.brightness = value;
        }, false);
        bind_scale(self, &self.controls.exposure_scale, &self.controls.exposure_value, |config, value| {
            config.exposure_value = value;
        }, false);
        bind_scale(self, &self.controls.contrast_scale, &self.controls.contrast_value, |config, value| {
            config.contrast = value;
        }, false);
        bind_scale(self, &self.controls.saturation_scale, &self.controls.saturation_value, |config, value| {
            config.saturation = value;
        }, false);
        bind_scale(self, &self.controls.hue_scale, &self.controls.hue_value, |config, value| {
            config.hue = value;
        }, false);
        bind_scale(self, &self.controls.temperature_scale, &self.controls.temperature_value, |config, value| {
            config.temperature = value;
        }, false);
        bind_scale(self, &self.controls.tint_scale, &self.controls.tint_value, |config, value| {
            config.tint = value;
        }, false);
        bind_scale(self, &self.controls.red_scale, &self.controls.red_value, |config, value| {
            config.red_gain = value;
        }, false);
        bind_scale(self, &self.controls.green_scale, &self.controls.green_value, |config, value| {
            config.green_gain = value;
        }, false);
        bind_scale(self, &self.controls.blue_scale, &self.controls.blue_value, |config, value| {
            config.blue_gain = value;
        }, false);
        bind_scale(self, &self.controls.gamma_scale, &self.controls.gamma_value, |config, value| {
            config.gamma = value;
        }, false);
        bind_scale(self, &self.controls.sharpness_scale, &self.controls.sharpness_value, |config, value| {
            config.sharpness = value;
        }, false);

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
                    app.set_status(&format!("Falha ao salvar configuracao: {error}"), true);
                    return;
                }
                app.set_status(
                    &format!("Configuracao salva em {}.", app.config_path.display()),
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

    fn sync_controls_from_state(&self) {
        let state = self.state.borrow();
        self.syncing_ui.set(true);

        self.controls.auto_apply_row.set_active(state.auto_apply);
        self.controls.show_grid_row.set_active(state.show_grid);
        self.controls.mirror_row.set_active(state.config.mirror);
        self.controls.record_audio_row.set_active(state.config.record_audio);
        self.controls
            .preset_row
            .set_selected(state.preset_index as u32);
        self.controls
            .audio_source_row
            .set_selected(selected_audio_index(&state.audio_sources, &state.config.audio_source));
        self.countdown_off_button
            .set_active(state.config.countdown_seconds == 0);
        self.countdown_three_button
            .set_active(state.config.countdown_seconds == 3);
        self.countdown_ten_button
            .set_active(state.config.countdown_seconds == 10);

        set_scale_value(&self.controls.brightness_scale, &self.controls.brightness_value, state.config.brightness);
        set_scale_value(&self.controls.exposure_scale, &self.controls.exposure_value, state.config.exposure_value);
        set_scale_value(&self.controls.contrast_scale, &self.controls.contrast_value, state.config.contrast);
        set_scale_value(&self.controls.saturation_scale, &self.controls.saturation_value, state.config.saturation);
        set_scale_value(&self.controls.hue_scale, &self.controls.hue_value, state.config.hue);
        set_scale_value(&self.controls.temperature_scale, &self.controls.temperature_value, state.config.temperature);
        set_scale_value(&self.controls.tint_scale, &self.controls.tint_value, state.config.tint);
        set_scale_value(&self.controls.red_scale, &self.controls.red_value, state.config.red_gain);
        set_scale_value(&self.controls.green_scale, &self.controls.green_value, state.config.green_gain);
        set_scale_value(&self.controls.blue_scale, &self.controls.blue_value, state.config.blue_gain);
        set_scale_value(&self.controls.gamma_scale, &self.controls.gamma_value, state.config.gamma);
        set_scale_value(&self.controls.sharpness_scale, &self.controls.sharpness_value, state.config.sharpness);
        self.refresh_zoom_controls();

        self.syncing_ui.set(false);
    }

    fn refresh_preview_chrome(&self) {
        let state = self.state.borrow();
        self.placeholder
            .set_visible(!state.preview_active && state.countdown_remaining.is_none());
        self.grid_overlay.set_visible(state.show_grid);
        self.preview_button.set_icon_name(if state.preview_active {
            "media-playback-stop-symbolic"
        } else {
            "media-playback-start-symbolic"
        });
        self.preview_button.set_tooltip_text(Some(if state.preview_active {
            "Parar preview"
        } else {
            "Iniciar preview"
        }));
    }

    fn refresh_capture_controls(&self) {
        let state = self.state.borrow();
        self.photo_mode_button
            .set_active(state.capture_mode == CaptureMode::Photo);
        self.video_mode_button
            .set_active(state.capture_mode == CaptureMode::Video);

        self.photo_mode_button.remove_css_class("camera-mode-button-active");
        self.video_mode_button.remove_css_class("camera-mode-button-active");
        self.capture_button.remove_css_class("capture-button-photo");
        self.capture_button.remove_css_class("capture-button-video");
        self.capture_button.remove_css_class("capture-button-recording");
        self.capture_button_glyph.remove_css_class("capture-button-glyph-photo");
        self.capture_button_glyph.remove_css_class("capture-button-glyph-video");
        self.capture_button_glyph.remove_css_class("capture-button-glyph-recording");

        match state.capture_mode {
            CaptureMode::Photo => {
                self.photo_mode_button.add_css_class("camera-mode-button-active");
                self.capture_button.add_css_class("capture-button-photo");
                self.capture_button_glyph
                    .add_css_class("capture-button-glyph-photo");
            }
            CaptureMode::Video if state.is_recording => {
                self.video_mode_button.add_css_class("camera-mode-button-active");
                self.capture_button.add_css_class("capture-button-recording");
                self.capture_button_glyph
                    .add_css_class("capture-button-glyph-recording");
            }
            CaptureMode::Video => {
                self.video_mode_button.add_css_class("camera-mode-button-active");
                self.capture_button.add_css_class("capture-button-video");
                self.capture_button_glyph
                    .add_css_class("capture-button-glyph-video");
            }
        }

        if state.countdown_remaining.is_some() {
            self.capture_button
                .set_tooltip_text(Some("Cancelar contagem regressiva"));
        } else {
            match state.capture_mode {
                CaptureMode::Photo => {
                    self.capture_button.set_tooltip_text(Some("Tirar foto"));
                }
                CaptureMode::Video if state.is_recording => {
                    self.capture_button.set_tooltip_text(Some("Parar gravação"));
                }
                CaptureMode::Video => {
                    self.capture_button.set_tooltip_text(Some("Iniciar gravação"));
                }
            }
        }
    }

    fn refresh_countdown_controls(&self) {
        let state = self.state.borrow();
        let configured_seconds = normalize_countdown_seconds(state.config.countdown_seconds);
        let countdown_remaining = state.countdown_remaining;

        if configured_seconds > 0 {
            self.countdown_button
                .set_tooltip_text(Some(&format!("Contagem regressiva: {configured_seconds}s")));
            self.countdown_button
                .add_css_class("camera-header-toggle-active");
        } else {
            self.countdown_button
                .set_tooltip_text(Some("Contagem regressiva"));
            self.countdown_button
                .remove_css_class("camera-header-toggle-active");
        }

        if let Some(remaining) = countdown_remaining {
            self.countdown_overlay_label.set_label(&remaining.to_string());
            self.countdown_overlay_label.set_visible(true);
        } else {
            self.countdown_overlay_label.set_visible(false);
        }
    }

    fn refresh_zoom_controls(&self) {
        let selected_index = self.state.borrow().config.resolution_index();
        let selected_option = preview_zoom_options()
            .get(selected_index)
            .or_else(|| preview_zoom_options().first());
        let selected_label = selected_option.map(|option| option.label.as_str()).unwrap_or("1x");

        self.zoom_label.set_label(selected_label);
        self.zoom_button
            .set_tooltip_text(Some(&format!("Zoom do preview: {selected_label}")));

        let was_syncing = self.syncing_ui.replace(true);
        for (index, button) in self.zoom_option_buttons.iter().enumerate() {
            let is_active = index == selected_index;
            button.set_active(is_active);
            if is_active {
                button.add_css_class("camera-zoom-choice-active");
            } else {
                button.remove_css_class("camera-zoom-choice-active");
            }
        }
        self.syncing_ui.set(was_syncing);
    }

    fn set_zoom_selector_expanded(&self, expanded: bool) {
        self.zoom_button.set_visible(!expanded);
        self.zoom_strip.set_visible(expanded);
    }

    fn refresh_header_metrics(&self) {
        self.title_widget.set_subtitle("");
    }

    fn persist_config(&self) -> Result<(), String> {
        self.state.borrow().config.save(&self.config_path)
    }

    fn set_status(&self, message: &str, toast: bool) {
        {
            let mut state = self.state.borrow_mut();
            state.status = message.to_string();
        }
        self.status_label.set_label(message);
        if toast {
            self.toast_overlay.add_toast(adw::Toast::new(message));
        }
    }

    fn on_config_changed(&self, restart_required: bool) {
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
                self.set_status(&format!("Falha ao salvar configuracao: {error}"), true);
                return;
            }

            if restart_required {
                self.set_status(
                    "Configuração salva. A nova resolução ou preset será aplicada no próximo preview.",
                    false,
                );
            } else {
                self.set_status("Configuração salva. Clique em Aplicar para enviar ao preview.", false);
            }
        }
    }

    fn apply_config_safely(&self, restart_required: bool) {
        if let Err(error) = self.persist_config() {
            self.set_status(&format!("Falha ao salvar configuracao: {error}"), true);
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
                    "Reiniciando o preview para aplicar a nova resolução ou preset...",
                    false,
                );
            } else {
                self.set_status(
                    "Nova resolução ou preset salvo. O próximo preview já abrirá com a nova configuração.",
                    false,
                );
            }
        } else {
            let _ = self.command_tx.send(WorkerCommand::ApplyConfig {
                config,
                restart: false,
            });
            self.set_status("Ajustes aplicados.", false);
        }
    }

    fn start_preview(&self) {
        self.cancel_countdown(None);
        if let Err(error) = self.persist_config() {
            self.set_status(&format!("Falha ao salvar configuracao: {error}"), true);
            return;
        }

        {
            let mut state = self.state.borrow_mut();
            state.preview_active = true;
            state.post_stop_status = None;
        }
        let _ = self.command_tx.send(WorkerCommand::StartPreview);
        self.set_status("Iniciando preview...", false);
        self.refresh_preview_chrome();
        self.refresh_header_metrics();
    }

    fn stop_preview(&self) {
        self.cancel_countdown(None);
        let _ = self.command_tx.send(WorkerCommand::StopPreview);
        self.set_status("Parando preview...", false);
    }

    fn capture_photo_now(&self) {
        let output_dir = photo_library_dir();
        if let Err(error) = fs::create_dir_all(&output_dir) {
            self.set_status(
                &format!("Falha ao preparar a pasta da câmera: {error}"),
                true,
            );
            return;
        }

        let output_path = output_dir.join(format!("camera-{}.jpg", timestamp()));
        let _ = self
            .command_tx
            .send(WorkerCommand::CapturePhoto { output_path: output_path.clone() });
        self.set_status(
            &format!(
                "Capturando foto em resolução máxima em {}...",
                output_path.display()
            ),
            false,
        );
    }

    fn start_recording_now(&self) {
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

    fn stop_recording_now(&self) {
        if !self.state.borrow().is_recording {
            return;
        }

        let _ = self.command_tx.send(WorkerCommand::StopRecording);
        self.state.borrow_mut().is_recording = false;
        self.refresh_capture_controls();
        self.set_status("Finalizando arquivo de vídeo...", false);
    }

    fn handle_capture_action(self: &Rc<Self>) {
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

    fn start_countdown(self: &Rc<Self>, action: PendingCaptureAction, seconds: u32) {
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

    fn on_countdown_tick(&self) -> ControlFlow {
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

    fn cancel_countdown(&self, message: Option<&str>) {
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

    fn set_countdown_seconds(&self, seconds: u32) {
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

    fn handle_worker_event(&self, event: WorkerEvent) {
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

    fn shutdown(&self) {
        if self.shutdown_sent.replace(true) {
            return;
        }
        self.cancel_countdown(None);
        let _ = self.command_tx.send(WorkerCommand::Shutdown);
        let _ = self.persist_config();
        let _ = fs::remove_file(&self.singleton_socket_path);
    }

    fn present_about_dialog(&self) {
        let app_name = localized_app_name();
        let dialog = adw::Dialog::builder()
            .title("Sobre")
            .content_width(520)
            .content_height(620)
            .build();
        let navigation_view = adw::NavigationView::new();
        navigation_view.set_animate_transitions(true);
        navigation_view.set_pop_on_escape(true);

        let header_title = adw::WindowTitle::new("Sobre", "");

        let back_button = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text("Voltar")
            .visible(false)
            .build();
        back_button.add_css_class("flat");

        let header_bar = adw::HeaderBar::new();
        header_bar.set_title_widget(Some(&header_title));
        header_bar.pack_start(&back_button);

        let details_subpage = build_about_details_subpage();
        let page = adw::PreferencesPage::builder()
            .name("about")
            .title("Sobre")
            .build();

        let summary_group = adw::PreferencesGroup::new();
        let summary_row = build_about_summary_row(app_name.as_str());
        summary_group.add(&summary_row);

        let author_row = adw::ActionRow::builder()
            .title("Caio Régis")
            .subtitle("@regiscaio")
            .build();
        author_row.set_activatable(false);
        summary_group.add(&author_row);

        let links_group = adw::PreferencesGroup::builder().title("Projeto").build();
        let website_row = self.build_uri_row("Página da web", "https://caioregis.com");
        let repository_row = self.build_uri_row(
            "Repositório do projeto",
            "https://github.com/regiscaio/fedora-galaxy-book-camera",
        );
        let issues_row = self.build_uri_row(
            "Relatar problema",
            "https://github.com/regiscaio/fedora-galaxy-book-camera/issues",
        );
        let details_row = build_suffix_action_row(
            "Detalhes",
            "Versão, app ID e caminhos usados pelo app.",
            "go-next-symbolic",
            "Abrir detalhes",
            {
                let navigation_view = navigation_view.clone();
                move || {
                    navigation_view.push_by_tag("details");
                }
            },
        );

        links_group.add(&website_row);
        links_group.add(&repository_row);
        links_group.add(&issues_row);
        links_group.add(&details_row);

        page.add(&summary_group);
        page.add(&links_group);

        let about_scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_width(0)
            .child(&page)
            .build();
        let about_page = adw::NavigationPage::with_tag(&about_scroller, "Sobre", "about");

        navigation_view.add(&about_page);
        navigation_view.add(&details_subpage);
        navigation_view.replace_with_tags(&["about"]);

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header_bar);
        toolbar_view.set_content(Some(&navigation_view));

        dialog.set_child(Some(&toolbar_view));

        back_button.connect_clicked({
            let navigation_view = navigation_view.clone();
            move |_| {
                navigation_view.pop();
            }
        });

        navigation_view.connect_visible_page_notify({
            let header_title = header_title.clone();
            let back_button = back_button.clone();
            move |navigation_view| {
                let Some(page) = navigation_view.visible_page() else {
                    header_title.set_title("Sobre");
                    back_button.set_visible(false);
                    return;
                };

                header_title.set_title(page.title().as_str());
                back_button.set_visible(navigation_view.previous_page(&page).is_some());
            }
        });

        dialog.present(Some(&self.window));
    }

    fn build_uri_row(&self, title: &str, uri: &'static str) -> adw::ActionRow {
        let window = self.window.clone();
        let toast_overlay = self.toast_overlay.clone();
        build_suffix_action_row(
            title,
            uri,
            "send-to-symbolic",
            "Abrir link",
            move || {
                let launcher = gtk::UriLauncher::new(uri);
                let toast_overlay = toast_overlay.clone();
                launcher.launch(
                    Some(&window),
                    None::<&gtk::gio::Cancellable>,
                    move |result| {
                        if let Err(error) = result {
                            toast_overlay.add_toast(adw::Toast::new(&format!(
                                "Falha ao abrir o link: {error}"
                            )));
                        }
                    },
                );
            },
        )
    }
}

fn countdown_status_message(action: PendingCaptureAction, seconds: u32) -> String {
    match action {
        PendingCaptureAction::Photo => format!("Foto em {seconds}s..."),
        PendingCaptureAction::StartRecording => format!("Vídeo em {seconds}s..."),
    }
}

fn apply_validated_startup_resolution(config: &mut CameraConfig) {
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

fn parse_args() -> CliArgs {
    let mut smoke_test = false;
    let mut capture_photo_once = None;
    let mut config_path = default_config_path();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke-test" => smoke_test = true,
            "--capture-photo-once" => {
                if let Some(path) = args.next() {
                    capture_photo_once = Some(PathBuf::from(path));
                }
            }
            "--config" => {
                if let Some(path) = args.next() {
                    config_path = PathBuf::from(path);
                }
            }
            _ => {}
        }
    }

    CliArgs {
        smoke_test,
        capture_photo_once,
        config_path,
    }
}

fn main() {
    let args = parse_args();

    if args.smoke_test {
        if let Err(error) = run_smoke_test(&args.config_path) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }

    let startup_config = CameraConfig::load(&args.config_path);
    if let Some(output_path) = args.capture_photo_once {
        match galaxybook_camera::capture_photo_max_resolution(&startup_config, &output_path) {
            Ok((width, height)) => {
                println!("photo={}", output_path.display());
                println!("resolution={}x{}", width, height);
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }

    set_softisp_env(&startup_config.softisp_mode);

    let singleton = match setup_singleton() {
        Ok(Some(singleton)) => singleton,
        Ok(None) => return,
        Err(error) => {
            eprintln!(
                "Falha ao preparar a instância única do {}: {error}",
                localized_app_name()
            );
            std::process::exit(1);
        }
    };

    let app = adw::Application::builder().application_id(APP_ID).build();
    let config_path = args.config_path.clone();
    let startup_config = Rc::new(startup_config);
    let singleton_holder = Rc::new(RefCell::new(Some(singleton)));

    app.connect_activate(move |app| {
        if app.active_window().is_some() {
            if let Some(window) = app.active_window() {
                window.present();
            }
            return;
        }

        let singleton = singleton_holder
            .borrow_mut()
            .take()
            .expect("singleton state should exist on first activation");
        let mut config = (*startup_config).clone();
        apply_validated_startup_resolution(&mut config);
        let window = CameraWindow::new(app, config_path.clone(), config, singleton);
        window.start_preview();
        window.window.present();
    });

    app.run();
}
