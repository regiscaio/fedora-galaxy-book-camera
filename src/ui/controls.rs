use galaxybook_camera::CaptureMode;
use gtk::prelude::*;
use libadwaita as adw;

pub fn refresh_preview_chrome(
    placeholder: &adw::StatusPage,
    grid_overlay: &gtk::DrawingArea,
    preview_button: &gtk::Button,
    preview_active: bool,
    countdown_active: bool,
    show_grid: bool,
) {
    placeholder.set_visible(!preview_active && !countdown_active);
    grid_overlay.set_visible(show_grid);
    preview_button.set_icon_name(if preview_active {
        "media-playback-stop-symbolic"
    } else {
        "media-playback-start-symbolic"
    });
    preview_button.set_tooltip_text(Some(if preview_active {
        "Parar preview"
    } else {
        "Iniciar preview"
    }));
}

pub fn refresh_capture_controls(
    photo_mode_button: &gtk::ToggleButton,
    video_mode_button: &gtk::ToggleButton,
    capture_button: &gtk::Button,
    capture_button_glyph: &gtk::Box,
    capture_mode: CaptureMode,
    is_recording: bool,
    countdown_active: bool,
) {
    photo_mode_button.set_active(capture_mode == CaptureMode::Photo);
    video_mode_button.set_active(capture_mode == CaptureMode::Video);

    photo_mode_button.remove_css_class("camera-mode-button-active");
    video_mode_button.remove_css_class("camera-mode-button-active");
    capture_button.remove_css_class("capture-button-photo");
    capture_button.remove_css_class("capture-button-video");
    capture_button.remove_css_class("capture-button-recording");
    capture_button_glyph.remove_css_class("capture-button-glyph-photo");
    capture_button_glyph.remove_css_class("capture-button-glyph-video");
    capture_button_glyph.remove_css_class("capture-button-glyph-recording");

    match capture_mode {
        CaptureMode::Photo => {
            photo_mode_button.add_css_class("camera-mode-button-active");
            capture_button.add_css_class("capture-button-photo");
            capture_button_glyph.add_css_class("capture-button-glyph-photo");
        }
        CaptureMode::Video if is_recording => {
            video_mode_button.add_css_class("camera-mode-button-active");
            capture_button.add_css_class("capture-button-recording");
            capture_button_glyph.add_css_class("capture-button-glyph-recording");
        }
        CaptureMode::Video => {
            video_mode_button.add_css_class("camera-mode-button-active");
            capture_button.add_css_class("capture-button-video");
            capture_button_glyph.add_css_class("capture-button-glyph-video");
        }
    }

    if countdown_active {
        capture_button.set_tooltip_text(Some("Cancelar contagem regressiva"));
    } else {
        match capture_mode {
            CaptureMode::Photo => {
                capture_button.set_tooltip_text(Some("Tirar foto"));
            }
            CaptureMode::Video if is_recording => {
                capture_button.set_tooltip_text(Some("Parar gravação"));
            }
            CaptureMode::Video => {
                capture_button.set_tooltip_text(Some("Iniciar gravação"));
            }
        }
    }
}

pub fn refresh_countdown_controls(
    countdown_button: &gtk::MenuButton,
    countdown_overlay_label: &gtk::Label,
    configured_seconds: u32,
    countdown_remaining: Option<u32>,
) {
    if configured_seconds > 0 {
        countdown_button
            .set_tooltip_text(Some(&format!("Contagem regressiva: {configured_seconds}s")));
        countdown_button.add_css_class("camera-header-toggle-active");
    } else {
        countdown_button.set_tooltip_text(Some("Contagem regressiva"));
        countdown_button.remove_css_class("camera-header-toggle-active");
    }

    if let Some(remaining) = countdown_remaining {
        countdown_overlay_label.set_label(&remaining.to_string());
        countdown_overlay_label.set_visible(true);
    } else {
        countdown_overlay_label.set_visible(false);
    }
}
