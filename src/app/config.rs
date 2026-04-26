use std::fs;
use std::path::Path;

use crate::{normalize_countdown_seconds, preview_zoom_options, tr};

const CONFIG_VALUE_MAX_LEN: usize = 240;

#[derive(Clone, Copy)]
pub enum Preset {
    Natural,
    Indoor,
    Daylight,
}

impl Preset {
    pub fn all() -> [Preset; 3] {
        [Preset::Natural, Preset::Indoor, Preset::Daylight]
    }

    pub fn translated_label(self) -> String {
        match self {
            Preset::Natural => tr("Natural"),
            Preset::Indoor => tr("Interno claro"),
            Preset::Daylight => tr("Luz do dia"),
        }
    }

    pub fn from_index(index: usize) -> Self {
        Self::all().get(index).copied().unwrap_or(Preset::Natural)
    }
}

#[derive(Clone)]
pub struct CameraConfig {
    pub softisp_mode: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub countdown_seconds: u32,
    pub show_grid: bool,
    pub mirror: bool,
    pub brightness: f64,
    pub exposure_value: f64,
    pub contrast: f64,
    pub saturation: f64,
    pub hue: f64,
    pub temperature: f64,
    pub tint: f64,
    pub red_gain: f64,
    pub green_gain: f64,
    pub blue_gain: f64,
    pub gamma: f64,
    pub sharpness: f64,
    pub record_audio: bool,
    pub audio_source: String,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            softisp_mode: "cpu".to_string(),
            width: Some(1920),
            height: Some(1092),
            countdown_seconds: 0,
            show_grid: true,
            mirror: false,
            brightness: 0.0,
            exposure_value: -0.04,
            contrast: 1.04,
            saturation: 1.05,
            hue: 0.00,
            temperature: 0.04,
            tint: 0.00,
            red_gain: 1.00,
            green_gain: 1.00,
            blue_gain: 1.00,
            gamma: 1.00,
            sharpness: 1.00,
            record_audio: true,
            audio_source: "default".to_string(),
        }
    }
}

impl CameraConfig {
    pub fn load(path: &Path) -> Self {
        let mut config = Self::default();
        let Ok(raw) = fs::read_to_string(path) else {
            return config;
        };

        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };

            match key.trim() {
                "LIBCAMERA_SOFTISP_MODE" => config.softisp_mode = value.trim().to_string(),
                "CAMERA_WIDTH" => config.width = parse_optional_u32(value),
                "CAMERA_HEIGHT" => config.height = parse_optional_u32(value),
                "CAMERA_COUNTDOWN" => {
                    config.countdown_seconds =
                        parse_countdown_seconds(value, config.countdown_seconds)
                }
                "CAMERA_SHOW_GRID" => {
                    config.show_grid = parse_bool(value, config.show_grid)
                }
                "CAMERA_MIRROR" => config.mirror = parse_bool(value, config.mirror),
                "CAMERA_BRIGHTNESS" => config.brightness = parse_f64(value, config.brightness),
                "CAMERA_EXPOSURE_VALUE" => {
                    config.exposure_value = parse_f64(value, config.exposure_value)
                }
                "CAMERA_CONTRAST" => config.contrast = parse_f64(value, config.contrast),
                "CAMERA_SATURATION" => config.saturation = parse_f64(value, config.saturation),
                "CAMERA_HUE" => config.hue = parse_f64(value, config.hue),
                "CAMERA_TEMPERATURE" => {
                    config.temperature = parse_f64(value, config.temperature)
                }
                "CAMERA_TINT" => config.tint = parse_f64(value, config.tint),
                "CAMERA_RED_GAIN" => config.red_gain = parse_f64(value, config.red_gain),
                "CAMERA_GREEN_GAIN" => {
                    config.green_gain = parse_f64(value, config.green_gain)
                }
                "CAMERA_BLUE_GAIN" => config.blue_gain = parse_f64(value, config.blue_gain),
                "CAMERA_GAMMA" => config.gamma = parse_f64(value, config.gamma),
                "CAMERA_SHARPNESS" => config.sharpness = parse_f64(value, config.sharpness),
                "CAMERA_RECORD_AUDIO" => {
                    config.record_audio = parse_bool(value, config.record_audio)
                }
                "CAMERA_AUDIO_SOURCE" => config.audio_source = value.trim().to_string(),
                _ => {}
            }
        }

        config.sanitize();
        config
    }

    pub fn sanitize(&mut self) {
        let defaults = Self::default();

        self.softisp_mode = sanitize_single_line(&self.softisp_mode, &defaults.softisp_mode);
        self.countdown_seconds = normalize_countdown_seconds(self.countdown_seconds);
        self.brightness = clamp_finite(self.brightness, -0.20, 0.25, defaults.brightness);
        self.exposure_value =
            clamp_finite(self.exposure_value, -0.50, 1.00, defaults.exposure_value);
        self.contrast = clamp_finite(self.contrast, 0.50, 2.00, defaults.contrast);
        self.saturation = clamp_finite(self.saturation, 0.00, 2.20, defaults.saturation);
        self.hue = clamp_finite(self.hue, -1.00, 1.00, defaults.hue);
        self.temperature = clamp_finite(self.temperature, -1.00, 1.00, defaults.temperature);
        self.tint = clamp_finite(self.tint, -1.00, 1.00, defaults.tint);
        self.red_gain = clamp_finite(self.red_gain, 0.50, 1.50, defaults.red_gain);
        self.green_gain = clamp_finite(self.green_gain, 0.50, 1.50, defaults.green_gain);
        self.blue_gain = clamp_finite(self.blue_gain, 0.50, 1.50, defaults.blue_gain);
        self.gamma = clamp_finite(self.gamma, 0.50, 1.80, defaults.gamma);
        self.sharpness = clamp_finite(self.sharpness, 1.00, 2.00, defaults.sharpness);
        self.audio_source = sanitize_single_line(&self.audio_source, &defaults.audio_source);
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }

        let mut config = self.clone();
        config.sanitize();

        let text = format!(
            "# Camera tuning used by:\n\
             #   galaxybook-camera (Rust + libcamera)\n\n\
             LIBCAMERA_SOFTISP_MODE={softisp_mode}\n\n\
             CAMERA_WIDTH={width}\n\
             CAMERA_HEIGHT={height}\n\n\
             CAMERA_COUNTDOWN={countdown_seconds}\n\
             CAMERA_SHOW_GRID={show_grid}\n\
             CAMERA_MIRROR={mirror}\n\
             CAMERA_BRIGHTNESS={brightness:.2}\n\
             CAMERA_EXPOSURE_VALUE={exposure_value:.2}\n\
             CAMERA_CONTRAST={contrast:.2}\n\
             CAMERA_SATURATION={saturation:.2}\n\
             CAMERA_HUE={hue:.2}\n\
             CAMERA_TEMPERATURE={temperature:.2}\n\
             CAMERA_TINT={tint:.2}\n\
             CAMERA_RED_GAIN={red_gain:.2}\n\
             CAMERA_GREEN_GAIN={green_gain:.2}\n\
             CAMERA_BLUE_GAIN={blue_gain:.2}\n\
             CAMERA_GAMMA={gamma:.2}\n\
             CAMERA_SHARPNESS={sharpness:.2}\n\
             CAMERA_RECORD_AUDIO={record_audio}\n\
             CAMERA_AUDIO_SOURCE={audio_source}\n",
            softisp_mode = config.softisp_mode,
            width = config.width.map(|value| value.to_string()).unwrap_or_default(),
            height = config.height.map(|value| value.to_string()).unwrap_or_default(),
            countdown_seconds = config.countdown_seconds,
            show_grid = config.show_grid,
            mirror = config.mirror,
            brightness = config.brightness,
            exposure_value = config.exposure_value,
            contrast = config.contrast,
            saturation = config.saturation,
            hue = config.hue,
            temperature = config.temperature,
            tint = config.tint,
            red_gain = config.red_gain,
            green_gain = config.green_gain,
            blue_gain = config.blue_gain,
            gamma = config.gamma,
            sharpness = config.sharpness,
            record_audio = config.record_audio,
            audio_source = config.audio_source,
        );

        fs::write(path, text).map_err(|err| err.to_string())
    }

    pub fn resolution_index(&self) -> usize {
        preview_zoom_options()
            .iter()
            .position(|option| Some(option.width) == self.width && Some(option.height) == self.height)
            .unwrap_or(0)
    }

    pub fn resolution_text(&self) -> String {
        match (self.width, self.height) {
            (Some(width), Some(height)) => format!("{width}x{height}"),
            _ => tr("automático"),
        }
    }

    pub fn zoom_text(&self) -> String {
        preview_zoom_options()
            .get(self.resolution_index())
            .map(|option| option.label.clone())
            .unwrap_or_else(|| "1x".to_string())
    }

    pub fn apply_preset(&mut self, preset: Preset) {
        match preset {
            Preset::Natural => {
                self.brightness = 0.00;
                self.exposure_value = -0.04;
                self.contrast = 1.04;
                self.saturation = 1.05;
                self.hue = 0.00;
                self.temperature = 0.04;
                self.tint = 0.00;
                self.red_gain = 1.00;
                self.green_gain = 1.00;
                self.blue_gain = 1.00;
                self.gamma = 1.00;
                self.sharpness = 1.00;
            }
            Preset::Indoor => {
                self.brightness = 0.11;
                self.exposure_value = 0.45;
                self.contrast = 1.12;
                self.saturation = 1.42;
                self.hue = 0.00;
                self.temperature = 0.10;
                self.tint = 0.00;
                self.red_gain = 1.03;
                self.green_gain = 1.00;
                self.blue_gain = 0.98;
                self.gamma = 1.08;
                self.sharpness = 1.10;
            }
            Preset::Daylight => {
                self.brightness = 0.05;
                self.exposure_value = 0.12;
                self.contrast = 1.12;
                self.saturation = 1.28;
                self.hue = 0.00;
                self.temperature = -0.06;
                self.tint = 0.00;
                self.red_gain = 0.99;
                self.green_gain = 1.00;
                self.blue_gain = 1.02;
                self.gamma = 1.02;
                self.sharpness = 1.10;
            }
        }
    }
}

fn parse_optional_u32(value: &str) -> Option<u32> {
    if value.trim().is_empty() {
        None
    } else {
        value.trim().parse().ok()
    }
}

fn parse_f64(value: &str, fallback: f64) -> f64 {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .unwrap_or(fallback)
}

fn parse_bool(value: &str, fallback: bool) -> bool {
    match value.trim() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => fallback,
    }
}

fn parse_countdown_seconds(value: &str, fallback: u32) -> u32 {
    value
        .trim()
        .parse::<u32>()
        .map(normalize_countdown_seconds)
        .unwrap_or(fallback)
}

fn clamp_finite(value: f64, min: f64, max: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

fn sanitize_single_line(value: &str, fallback: &str) -> String {
    let sanitized = value
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(CONFIG_VALUE_MAX_LEN)
        .collect::<String>();

    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_non_finite_values_and_clamps_to_ui_ranges() {
        let mut config = CameraConfig {
            brightness: f64::NAN,
            exposure_value: 9.0,
            contrast: -4.0,
            saturation: 8.0,
            hue: -8.0,
            temperature: 4.0,
            tint: f64::INFINITY,
            red_gain: 0.0,
            green_gain: 3.0,
            blue_gain: f64::NEG_INFINITY,
            gamma: 0.0,
            sharpness: 9.0,
            ..CameraConfig::default()
        };

        config.sanitize();

        assert_eq!(config.brightness, CameraConfig::default().brightness);
        assert_eq!(config.exposure_value, 1.0);
        assert_eq!(config.contrast, 0.5);
        assert_eq!(config.saturation, 2.2);
        assert_eq!(config.hue, -1.0);
        assert_eq!(config.temperature, 1.0);
        assert_eq!(config.tint, CameraConfig::default().tint);
        assert_eq!(config.red_gain, 0.5);
        assert_eq!(config.green_gain, 1.5);
        assert_eq!(config.blue_gain, CameraConfig::default().blue_gain);
        assert_eq!(config.gamma, 0.5);
        assert_eq!(config.sharpness, 2.0);
    }

    #[test]
    fn sanitize_keeps_config_values_single_line() {
        let mut config = CameraConfig {
            softisp_mode: " cpu\nCAMERA_BRIGHTNESS=9 ".to_string(),
            audio_source: " default\nCAMERA_RECORD_AUDIO=false ".to_string(),
            ..CameraConfig::default()
        };

        config.sanitize();

        assert_eq!(config.softisp_mode, "cpu");
        assert_eq!(config.audio_source, "default");
    }
}
