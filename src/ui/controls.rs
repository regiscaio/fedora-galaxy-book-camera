use std::cell::Cell;

use galaxybook_camera::{tr, trf, AudioSourceOption, CameraConfig, CaptureMode};
use gtk::prelude::*;
use libadwaita::prelude::ComboRowExt;
use libadwaita as adw;

use super::{selected_audio_index, set_scale_value, refresh_zoom_selector, ControlWidgets};

pub struct ControlStateSnapshot {
    pub config: CameraConfig,
    pub auto_apply: bool,
    pub show_grid: bool,
    pub preset_index: usize,
    pub audio_sources: Vec<AudioSourceOption>,
}

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
    let tooltip = if preview_active {
        tr("Parar preview")
    } else {
        tr("Iniciar preview")
    };
    preview_button.set_tooltip_text(Some(&tooltip));
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
        capture_button.set_tooltip_text(Some(&tr("Cancelar contagem regressiva")));
    } else {
        match capture_mode {
            CaptureMode::Photo => {
                capture_button.set_tooltip_text(Some(&tr("Tirar foto")));
            }
            CaptureMode::Video if is_recording => {
                capture_button.set_tooltip_text(Some(&tr("Parar gravação")));
            }
            CaptureMode::Video => {
                capture_button.set_tooltip_text(Some(&tr("Iniciar gravação")));
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
            .set_tooltip_text(Some(&trf(
                "Contagem regressiva: {seconds}s",
                &[("seconds", configured_seconds.to_string())],
            )));
        countdown_button.add_css_class("camera-header-toggle-active");
    } else {
        countdown_button.set_tooltip_text(Some(&tr("Contagem regressiva")));
        countdown_button.remove_css_class("camera-header-toggle-active");
    }

    if let Some(remaining) = countdown_remaining {
        countdown_overlay_label.set_label(&remaining.to_string());
        countdown_overlay_label.set_visible(true);
    } else {
        countdown_overlay_label.set_visible(false);
    }
}

pub fn sync_controls_from_state(
    controls: &ControlWidgets,
    countdown_off_button: &gtk::CheckButton,
    countdown_three_button: &gtk::CheckButton,
    countdown_five_button: &gtk::CheckButton,
    countdown_ten_button: &gtk::CheckButton,
    zoom_button: &gtk::Button,
    zoom_label: &gtk::Label,
    zoom_option_buttons: &[gtk::ToggleButton],
    syncing_ui: &Cell<bool>,
    state: &ControlStateSnapshot,
) {
    syncing_ui.set(true);

    controls.auto_apply_row.set_active(state.auto_apply);
    controls.show_grid_row.set_active(state.show_grid);
    controls.mirror_row.set_active(state.config.mirror);
    controls
        .record_audio_row
        .set_active(state.config.record_audio);
    controls.preset_row.set_selected(state.preset_index as u32);
    controls.audio_source_row.set_selected(selected_audio_index(
        &state.audio_sources,
        &state.config.audio_source,
    ));
    countdown_off_button.set_active(state.config.countdown_seconds == 0);
    countdown_three_button.set_active(state.config.countdown_seconds == 3);
    countdown_five_button.set_active(state.config.countdown_seconds == 5);
    countdown_ten_button.set_active(state.config.countdown_seconds == 10);

    set_scale_value(
        &controls.brightness_scale,
        &controls.brightness_value,
        state.config.brightness,
    );
    set_scale_value(
        &controls.exposure_scale,
        &controls.exposure_value,
        state.config.exposure_value,
    );
    set_scale_value(
        &controls.contrast_scale,
        &controls.contrast_value,
        state.config.contrast,
    );
    set_scale_value(
        &controls.saturation_scale,
        &controls.saturation_value,
        state.config.saturation,
    );
    set_scale_value(&controls.hue_scale, &controls.hue_value, state.config.hue);
    set_scale_value(
        &controls.temperature_scale,
        &controls.temperature_value,
        state.config.temperature,
    );
    set_scale_value(
        &controls.tint_scale,
        &controls.tint_value,
        state.config.tint,
    );
    set_scale_value(
        &controls.red_scale,
        &controls.red_value,
        state.config.red_gain,
    );
    set_scale_value(
        &controls.green_scale,
        &controls.green_value,
        state.config.green_gain,
    );
    set_scale_value(
        &controls.blue_scale,
        &controls.blue_value,
        state.config.blue_gain,
    );
    set_scale_value(
        &controls.gamma_scale,
        &controls.gamma_value,
        state.config.gamma,
    );
    set_scale_value(
        &controls.sharpness_scale,
        &controls.sharpness_value,
        state.config.sharpness,
    );
    refresh_zoom_selector(
        state.config.resolution_index(),
        zoom_button,
        zoom_label,
        zoom_option_buttons,
        syncing_ui,
    );

    syncing_ui.set(false);
}
