mod bindings;
mod capture;
mod events;

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Sender;

use adw::prelude::*;
use galaxybook_camera::{
    detect_audio_sources,
    localized_app_name,
    normalize_countdown_seconds,
    preferred_video_encoder_backend,
    preview_zoom_options,
    spawn_camera_worker,
    AudioSourceOption,
    CameraConfig,
    CaptureMode,
    OwnedFrame,
    SingletonState,
    WorkerCommand,
    WorkerEvent,
};
use gtk::glib;
use gtk::prelude::*;
use gtk::{Align, Orientation};
use libadwaita as adw;

use super::{
    apply_application_css,
    build_control_widgets,
    build_sidebar,
    build_zoom_selector,
    ControlStateSnapshot,
    draw_preview_grid,
    present_about_dialog,
    refresh_capture_controls,
    refresh_countdown_controls,
    refresh_preview_chrome,
    refresh_zoom_selector,
    set_zoom_selector_expanded,
    sync_controls_from_state,
    ControlWidgets,
};

const WINDOW_WIDTH: i32 = 1320;
const WINDOW_HEIGHT: i32 = 880;

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

pub struct CameraWindow {
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
    pub fn new(
        app: &adw::Application,
        config_path: PathBuf,
        mut config: CameraConfig,
        singleton: SingletonState,
    ) -> Rc<Self> {
        apply_application_css();
        apply_validated_startup_resolution(&mut config);
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

    pub fn start_preview(&self) {
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

    pub fn present(&self) {
        self.window.present();
    }

    fn sync_controls_from_state(&self) {
        let snapshot = {
            let state = self.state.borrow();
            ControlStateSnapshot {
                config: state.config.clone(),
                auto_apply: state.auto_apply,
                show_grid: state.show_grid,
                preset_index: state.preset_index,
                audio_sources: state.audio_sources.clone(),
            }
        };

        sync_controls_from_state(
            &self.controls,
            &self.countdown_off_button,
            &self.countdown_three_button,
            &self.countdown_ten_button,
            &self.zoom_button,
            &self.zoom_label,
            &self.zoom_option_buttons,
            &self.syncing_ui,
            &snapshot,
        );
    }

    fn refresh_preview_chrome(&self) {
        let state = self.state.borrow();
        refresh_preview_chrome(
            &self.placeholder,
            &self.grid_overlay,
            &self.preview_button,
            state.preview_active,
            state.countdown_remaining.is_some(),
            state.show_grid,
        );
    }

    fn refresh_capture_controls(&self) {
        let state = self.state.borrow();
        refresh_capture_controls(
            &self.photo_mode_button,
            &self.video_mode_button,
            &self.capture_button,
            &self.capture_button_glyph,
            state.capture_mode,
            state.is_recording,
            state.countdown_remaining.is_some(),
        );
    }

    fn refresh_countdown_controls(&self) {
        let state = self.state.borrow();
        refresh_countdown_controls(
            &self.countdown_button,
            &self.countdown_overlay_label,
            normalize_countdown_seconds(state.config.countdown_seconds),
            state.countdown_remaining,
        );
    }

    fn refresh_zoom_controls(&self) {
        refresh_zoom_selector(
            self.state.borrow().config.resolution_index(),
            &self.zoom_button,
            &self.zoom_label,
            &self.zoom_option_buttons,
            &self.syncing_ui,
        );
    }

    fn set_zoom_selector_expanded(&self, expanded: bool) {
        set_zoom_selector_expanded(&self.zoom_button, &self.zoom_strip, expanded);
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

    fn shutdown(&self) {
        if self.shutdown_sent.replace(true) {
            return;
        }
        self.cancel_countdown(None);
        let _ = self.command_tx.send(WorkerCommand::Shutdown);
        let _ = self.persist_config();
        let _ = fs::remove_file(&self.singleton_socket_path);
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
