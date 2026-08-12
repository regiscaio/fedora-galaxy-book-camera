use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;

use libcamera::{
    camera_manager::CameraManager,
    properties::Model,
};

mod app;
mod camera;
mod image;
mod media;
mod updates;

pub use app::config::{CameraConfig, Preset};
pub use app::localization::{
    init_i18n,
    localized_app_name,
    localized_app_name_for_locale,
    localized_camera_word_for_locale,
    tr,
    trf,
    trn,
};
pub use app::paths::{default_config_path, photo_library_dir, timestamp, video_library_dir};
pub use app::singleton::{setup_singleton, SingletonState};
pub use camera::capture::capture_photo_max_resolution;
pub use camera::worker::{spawn_camera_worker, WorkerCommand, WorkerEvent};
pub use media::audio::{
    detect_audio_sources,
    selected_audio_source_label,
    AudioSourceOption,
};
pub use media::ffmpeg::{preferred_video_encoder_backend, VideoEncoderBackend};
pub use updates::{install_package_updates, package_update_names};
pub(crate) use camera::capture::{
    capture_still_frame_with_manager,
    capture_photo_max_resolution_with_manager,
    write_photo_from_frame,
};
pub(crate) use image::adjustments::{apply_adjustments, AdjustmentProfile};
pub(crate) use media::ffmpeg::{
    spawn_video_recorder,
    write_smoke_video,
    RecorderHandle,
};

pub const APP_ID: &str = "com.caioregis.GalaxyBookCamera";
pub const APP_NAME: &str = "Galaxy Book Camera";
#[cfg(test)]
const DEV_TUNING_PATH_RELATIVE: &str = "data/libcamera/simple/ov02c10.yaml";
#[cfg(test)]
const SYSTEM_TUNING_PATH: &str = "/usr/share/libcamera/ipa/simple/ov02c10.yaml";
const COUNTDOWN_OPTIONS: [u32; 4] = [0, 3, 5, 10];
const PREVIEW_FRAMERATE: u32 = 30;
const MAX_PREVIEW_LONG_EDGE: usize = 1280;
const STILL_CAPTURE_WARMUP_FRAMES: u32 = 6;
const DRM_RENDER_NODES: [&str; 2] = ["/dev/dri/renderD128", "/dev/dri/renderD129"];
static PREVIEW_CAPABILITIES: OnceLock<RwLock<PreviewCapabilities>> = OnceLock::new();
const PREVIEW_ZOOM_PRESETS: [(f64, &str); 5] = [
    (1.0, "1x"),
    (2.0, "2x"),
    (3.0, "3x"),
    (5.0, "5x"),
    (10.0, "10x"),
];
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Photo,
    Video,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolutionOption {
    pub label: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreviewZoomOption {
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub factor: f64,
}

#[derive(Clone)]
struct PreviewCapabilities {
    resolutions: Vec<ResolutionOption>,
}

fn format_resolution_label(width: u32, height: u32) -> String {
    format!("{width} x {height}")
}

fn resolution_option(width: u32, height: u32) -> ResolutionOption {
    ResolutionOption {
        label: format_resolution_label(width, height),
        width,
        height,
    }
}

fn derive_preview_zoom_options_from(options: &[ResolutionOption]) -> Vec<PreviewZoomOption> {
    let Some(base_option) = options.first() else {
        return Vec::new();
    };
    let base_width = base_option.width.max(2);
    let base_height = base_option.height.max(2);

    PREVIEW_ZOOM_PRESETS
        .iter()
        .map(|(factor, label)| {
            let width = ((base_width as f64) / factor).round().max(2.0) as u32;
            let height = ((base_height as f64) / factor).round().max(2.0) as u32;

            PreviewZoomOption {
                label: (*label).to_string(),
                width: width - (width % 2),
                height: height - (height % 2),
                factor: *factor,
            }
        })
        .collect()
}

fn fallback_preview_resolution_options() -> Vec<ResolutionOption> {
    vec![resolution_option(1920, 1092), resolution_option(1280, 720)]
}

fn sorted_resolution_dimensions(
    discrete_sizes: impl IntoIterator<Item = (u32, u32)>,
    range_max: Option<(u32, u32)>,
) -> Vec<(u32, u32)> {
    let mut sizes: Vec<(u32, u32)> = discrete_sizes
        .into_iter()
        .filter(|(width, height)| *width > 0 && *height > 0)
        .collect();
    if let Some((width, height)) = range_max.filter(|(width, height)| *width > 0 && *height > 0) {
        sizes.push((width, height));
    }
    sizes.sort_unstable_by(|left, right| {
        let left_area = left.0 as u64 * left.1 as u64;
        let right_area = right.0 as u64 * right.1 as u64;
        right_area
            .cmp(&left_area)
            .then_with(|| right.0.cmp(&left.0))
            .then_with(|| right.1.cmp(&left.1))
    });
    sizes.dedup();
    sizes
}

fn preferred_camera_id_from(cameras: &[(String, String)]) -> Option<String> {
    cameras
        .iter()
        .find(|(id, model)| {
            id.to_ascii_lowercase().contains("ov02c10")
                || model.to_ascii_lowercase().contains("ov02c10")
        })
        .or_else(|| cameras.first())
        .map(|(id, _)| id.clone())
}

pub(crate) fn preferred_camera_id(manager: &CameraManager) -> Option<String> {
    let cameras = manager.cameras();
    if cameras.is_empty() {
        return None;
    }
    let cameras = cameras
        .iter()
        .map(|camera| {
            let model = camera
                .properties()
                .get::<Model>()
                .map(|model| model.0)
                .unwrap_or_default();
            (camera.id().to_string(), model)
        })
        .collect::<Vec<_>>();
    preferred_camera_id_from(&cameras)
}

fn preview_capabilities() -> &'static RwLock<PreviewCapabilities> {
    PREVIEW_CAPABILITIES.get_or_init(|| {
        RwLock::new(PreviewCapabilities {
            resolutions: fallback_preview_resolution_options(),
        })
    })
}

pub(crate) fn refresh_preview_capabilities(
    discrete_sizes: impl IntoIterator<Item = (u32, u32)>,
    range_max: Option<(u32, u32)>,
) -> Result<(), String> {
    let resolutions = sorted_resolution_dimensions(discrete_sizes, range_max)
        .into_iter()
        .map(|(width, height)| resolution_option(width, height))
        .collect::<Vec<_>>();
    if resolutions.is_empty() {
        return Err(tr("O libcamera não retornou resoluções válidas para o preview."));
    }
    let mut capabilities = preview_capabilities()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    capabilities.resolutions = resolutions;
    Ok(())
}

pub fn preview_resolution_options() -> Vec<ResolutionOption> {
    preview_capabilities()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .resolutions
        .clone()
}

pub fn preview_zoom_options() -> Vec<PreviewZoomOption> {
    derive_preview_zoom_options_from(&preview_resolution_options())
}

#[derive(Clone)]
pub struct OwnedFrame {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

fn validate_strided_rgba_layout(
    width: usize,
    height: usize,
    stride: usize,
    source_len: usize,
) -> Result<(usize, usize, usize), String> {
    if width == 0 || height == 0 {
        return Err(tr("Frame inválido: dados insuficientes para a resolução atual."));
    }

    let row_bytes = width
        .checked_mul(4)
        .ok_or_else(|| tr("Frame inválido: overflow ao calcular o tamanho do buffer."))?;
    if stride < row_bytes {
        return Err(tr("Frame inválido: dados insuficientes para a resolução atual."));
    }

    let last_row_offset = (height - 1)
        .checked_mul(stride)
        .ok_or_else(|| tr("Frame inválido: overflow ao calcular o tamanho do buffer."))?;
    let required = last_row_offset
        .checked_add(row_bytes)
        .ok_or_else(|| tr("Frame inválido: overflow ao calcular o tamanho do buffer."))?;
    if source_len < required {
        return Err(tr("Frame inválido: dados insuficientes para a resolução atual."));
    }

    let packed_len = row_bytes
        .checked_mul(height)
        .ok_or_else(|| tr("Frame inválido: overflow ao calcular o tamanho do buffer."))?;
    Ok((row_bytes, required, packed_len))
}

impl OwnedFrame {
    fn from_strided_rgba(width: usize, height: usize, stride: usize, source: &[u8]) -> Result<Self, String> {
        let (row_bytes, required, packed_len) =
            validate_strided_rgba_layout(width, height, stride, source.len())?;

        let mut data = if stride == row_bytes {
            source[..required].to_vec()
        } else {
            let mut packed = vec![0_u8; packed_len];
            for row in 0..height {
                let src_start = row * stride;
                let src_end = src_start + row_bytes;
                let dst_start = row * row_bytes;
                let dst_end = dst_start + row_bytes;
                packed[dst_start..dst_end].copy_from_slice(&source[src_start..src_end]);
            }
            packed
        };

        for pixel in data.chunks_exact_mut(4) {
            pixel[3] = 255;
        }

        Ok(Self { width, height, data })
    }

    fn from_strided_rgba_scaled(
        source_width: usize,
        source_height: usize,
        stride: usize,
        source: &[u8],
        target_width: usize,
        target_height: usize,
    ) -> Result<Self, String> {
        if target_width == source_width && target_height == source_height {
            return Self::from_strided_rgba(source_width, source_height, stride, source);
        }

        validate_strided_rgba_layout(source_width, source_height, stride, source.len())?;
        if target_width == 0 || target_height == 0 {
            return Err(tr("Frame inválido: dados insuficientes para a resolução atual."));
        }
        let target_pixels = target_width
            .checked_mul(target_height)
            .ok_or_else(|| tr("Frame inválido: overflow ao calcular o tamanho do buffer."))?;
        let target_len = target_pixels
            .checked_mul(4)
            .ok_or_else(|| tr("Frame inválido: overflow ao calcular o tamanho do buffer."))?;

        let mut data = vec![0_u8; target_len];
        for target_y in 0..target_height {
            let source_y = target_y * source_height / target_height;
            let source_row_start = source_y * stride;
            let target_row_start = target_y * target_width * 4;
            for target_x in 0..target_width {
                let source_x = target_x * source_width / target_width;
                let source_index = source_row_start + source_x * 4;
                let target_index = target_row_start + target_x * 4;
                data[target_index..(target_index + 3)].copy_from_slice(&source[source_index..(source_index + 3)]);
                data[target_index + 3] = 255;
            }
        }

        Ok(Self {
            width: target_width,
            height: target_height,
            data,
        })
    }

    fn scaled_nearest(&self, target_width: usize, target_height: usize) -> Self {
        if target_width == self.width && target_height == self.height {
            return self.clone();
        }

        let mut data = vec![0_u8; target_width * target_height * 4];
        for target_y in 0..target_height {
            let source_y = target_y * self.height / target_height;
            let target_row_start = target_y * target_width * 4;
            let source_row_start = source_y * self.width * 4;
            for target_x in 0..target_width {
                let source_x = target_x * self.width / target_width;
                let source_index = source_row_start + source_x * 4;
                let target_index = target_row_start + target_x * 4;
                data[target_index..(target_index + 4)]
                    .copy_from_slice(&self.data[source_index..(source_index + 4)]);
            }
        }

        Self {
            width: target_width,
            height: target_height,
            data,
        }
    }
}

fn preview_dimensions(width: usize, height: usize) -> (usize, usize) {
    let long_edge = width.max(height);
    if long_edge <= MAX_PREVIEW_LONG_EDGE {
        return (width, height);
    }

    let scale = MAX_PREVIEW_LONG_EDGE as f64 / long_edge as f64;
    let scaled_width = ((width as f64) * scale).round().max(1.0) as usize;
    let scaled_height = ((height as f64) * scale).round().max(1.0) as usize;
    (scaled_width, scaled_height)
}

pub fn normalize_softisp_mode(mode: &str) -> String {
    match mode.trim().to_ascii_lowercase().as_str() {
        "gpu" => "gpu".to_string(),
        _ => "cpu".to_string(),
    }
}

/// Configure libcamera before GTK, the camera worker, or any other threads start.
pub fn configure_libcamera_environment(mode: &str) {
    let mode = normalize_softisp_mode(mode);
    unsafe {
        std::env::set_var("LIBCAMERA_SOFTISP_MODE", mode);
    }
}

pub fn normalize_countdown_seconds(value: u32) -> u32 {
    if COUNTDOWN_OPTIONS.contains(&value) {
        value
    } else {
        0
    }
}

pub fn countdown_options() -> &'static [u32] {
    &COUNTDOWN_OPTIONS
}

fn synthetic_smoke_frame() -> OwnedFrame {
    let width = 320;
    let height = 180;
    let mut data = vec![0_u8; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let offset = (y * width + x) * 4;
            data[offset] = ((x * 255) / width) as u8;
            data[offset + 1] = ((y * 255) / height) as u8;
            data[offset + 2] = 180;
            data[offset + 3] = 255;
        }
    }

    OwnedFrame { width, height, data }
}

fn require_camera_count(cameras_count: usize) -> Result<(), String> {
    if cameras_count == 0 {
        Err(tr("Nenhuma câmera disponível para o teste físico."))
    } else {
        Ok(())
    }
}

struct SmokeArtifacts {
    directory: PathBuf,
}

impl SmokeArtifacts {
    fn create(test_name: &str) -> Result<Self, String> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let base = env::temp_dir();

        for attempt in 0_u8..32 {
            let directory = base.join(format!(
                "galaxybook-camera-{test_name}-{}-{timestamp}-{attempt}",
                std::process::id(),
            ));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            builder.mode(0o700);
            match builder.create(&directory) {
                Ok(()) => return Ok(Self { directory }),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.to_string()),
            }
        }

        Err(tr("Não foi possível criar diretório temporário exclusivo para o teste."))
    }

    fn photo_path(&self) -> PathBuf {
        self.directory.join("photo.jpg")
    }

    fn video_path(&self) -> PathBuf {
        self.directory.join("video.mp4")
    }
}

impl Drop for SmokeArtifacts {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

pub fn run_self_test(config_path: &Path) -> Result<(), String> {
    let config = CameraConfig::load(config_path);
    let artifacts = SmokeArtifacts::create("self-test")?;
    let photo_path = artifacts.photo_path();
    let video_path = artifacts.video_path();
    let mut frame = synthetic_smoke_frame();
    let profile = AdjustmentProfile::new(&config);
    apply_adjustments(&mut frame, &profile);

    write_photo_from_frame(&frame, &photo_path)?;
    write_smoke_video(&frame, &video_path)?;

    println!("config={}", config_path.display());
    println!("source=synthetic");
    println!("resolution={}", config.resolution_text());
    println!("photo=verified");
    println!("video=verified");
    Ok(())
}

pub fn run_smoke_test(config_path: &Path) -> Result<(), String> {
    let config = CameraConfig::load(config_path);
    let manager = CameraManager::new().map_err(|error| {
        trf(
            "Falha ao inicializar libcamera no teste físico: {error}",
            &[("error", error.to_string())],
        )
    })?;
    let cameras_count = manager.cameras().iter().count();
    require_camera_count(cameras_count)?;

    let artifacts = SmokeArtifacts::create("smoke-test")?;
    let photo_path = artifacts.photo_path();
    let video_path = artifacts.video_path();
    let mut frame = capture_still_frame_with_manager(
        &manager,
        0,
        std::time::Duration::from_secs(8),
    )?;
    let profile = AdjustmentProfile::new(&config);
    apply_adjustments(&mut frame, &profile);
    write_photo_from_frame(&frame, &photo_path)?;
    write_smoke_video(&frame, &video_path)?;

    println!("config={}", config_path.display());
    println!("source=hardware");
    println!("cameras={cameras_count}");
    println!("captured={}x{}", frame.width, frame.height);
    println!("photo=verified");
    println!("video=verified");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn neutral_config() -> CameraConfig {
        CameraConfig {
            brightness: 0.0,
            exposure_value: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            hue: 0.0,
            temperature: 0.0,
            tint: 0.0,
            red_gain: 1.0,
            green_gain: 1.0,
            blue_gain: 1.0,
            gamma: 1.0,
            sharpness: 1.0,
            mirror: false,
            ..CameraConfig::default()
        }
    }

    fn frame_from_pixels(width: usize, height: usize, pixels: &[(u8, u8, u8)]) -> OwnedFrame {
        let mut data = Vec::with_capacity(width * height * 4);
        for (red, green, blue) in pixels {
            data.extend_from_slice(&[*red, *green, *blue, 255]);
        }

        OwnedFrame { width, height, data }
    }

    fn rgb_at(frame: &OwnedFrame, x: usize, y: usize) -> (u8, u8, u8) {
        let index = (y * frame.width + x) * 4;
        (frame.data[index], frame.data[index + 1], frame.data[index + 2])
    }

    #[test]
    fn brightness_adjustment_changes_pixel_values() {
        let mut config = neutral_config();
        config.brightness = 0.20;

        let mut frame = frame_from_pixels(1, 1, &[(90, 100, 110)]);
        let profile = AdjustmentProfile::new(&config);
        apply_adjustments(&mut frame, &profile);

        let (red, green, blue) = rgb_at(&frame, 0, 0);
        assert!(red > 90);
        assert!(green > 100);
        assert!(blue > 110);
    }

    #[test]
    fn contrast_adjustment_expands_shadow_and_highlight() {
        let mut config = neutral_config();
        config.contrast = 1.40;

        let mut frame = frame_from_pixels(2, 1, &[(64, 64, 64), (192, 192, 192)]);
        let profile = AdjustmentProfile::new(&config);
        apply_adjustments(&mut frame, &profile);

        let dark = rgb_at(&frame, 0, 0).0;
        let bright = rgb_at(&frame, 1, 0).0;
        assert!(dark < 64);
        assert!(bright > 192);
    }

    #[test]
    fn saturation_adjustment_changes_channel_separation() {
        let mut config = neutral_config();
        config.saturation = 0.40;

        let original = (210_u8, 120_u8, 40_u8);
        let mut frame = frame_from_pixels(1, 1, &[original]);
        let profile = AdjustmentProfile::new(&config);
        apply_adjustments(&mut frame, &profile);

        let adjusted = rgb_at(&frame, 0, 0);
        let original_span = original.0.max(original.1).max(original.2) - original.0.min(original.1).min(original.2);
        let adjusted_span = adjusted.0.max(adjusted.1).max(adjusted.2) - adjusted.0.min(adjusted.1).min(adjusted.2);
        assert!(adjusted_span < original_span);
    }

    #[test]
    fn deep_shadow_casts_are_neutralized() {
        let config = neutral_config();

        let original = (18_u8, 36_u8, 24_u8);
        let mut frame = frame_from_pixels(1, 1, &[original]);
        let profile = AdjustmentProfile::new(&config);
        apply_adjustments(&mut frame, &profile);

        let adjusted = rgb_at(&frame, 0, 0);
        let original_span =
            original.0.max(original.1).max(original.2) - original.0.min(original.1).min(original.2);
        let adjusted_span =
            adjusted.0.max(adjusted.1).max(adjusted.2) - adjusted.0.min(adjusted.1).min(adjusted.2);

        assert!(adjusted_span < original_span);
    }

    #[test]
    fn sharpness_adjustment_changes_center_pixel() {
        let mut config = neutral_config();
        config.sharpness = 2.0;

        let mut frame = frame_from_pixels(
            3,
            3,
            &[
                (100, 100, 100),
                (100, 100, 100),
                (100, 100, 100),
                (100, 100, 100),
                (140, 140, 140),
                (100, 100, 100),
                (100, 100, 100),
                (100, 100, 100),
                (100, 100, 100),
            ],
        );
        let profile = AdjustmentProfile::new(&config);
        apply_adjustments(&mut frame, &profile);

        let center = rgb_at(&frame, 1, 1).0;
        assert!(center > 140);
    }

    #[test]
    fn normalize_ffmpeg_stderr_ignores_libva_info() {
        let normalized = media::ffmpeg::normalize_ffmpeg_stderr(
            b"libva info: VA-API version 1.23.0\nreal warning\nlibva info: driver loaded\n",
        );
        assert_eq!(normalized, "real warning");
    }

    #[test]
    fn preview_dimensions_downscale_large_stream_preserving_aspect_ratio() {
        let (width, height) = preview_dimensions(1920, 1092);
        assert_eq!(width, 1280);
        assert_eq!(height, 728);
    }

    #[test]
    fn scaled_preview_samples_expected_pixels() {
        let frame = frame_from_pixels(
            4,
            2,
            &[
                (10, 0, 0),
                (20, 0, 0),
                (30, 0, 0),
                (40, 0, 0),
                (50, 0, 0),
                (60, 0, 0),
                (70, 0, 0),
                (80, 0, 0),
            ],
        );

        let scaled = frame.scaled_nearest(2, 1);
        assert_eq!(scaled.width, 2);
        assert_eq!(scaled.height, 1);
        assert_eq!(rgb_at(&scaled, 0, 0), (10, 0, 0));
        assert_eq!(rgb_at(&scaled, 1, 0), (30, 0, 0));
    }

    #[test]
    fn strided_rgba_rejects_zero_dimensions() {
        assert!(OwnedFrame::from_strided_rgba(0, 1, 4, &[]).is_err());
        assert!(OwnedFrame::from_strided_rgba(1, 0, 4, &[]).is_err());
        assert!(OwnedFrame::from_strided_rgba_scaled(0, 1, 4, &[], 1, 1).is_err());
        assert!(OwnedFrame::from_strided_rgba_scaled(1, 1, 4, &[0; 4], 0, 1).is_err());
        assert!(OwnedFrame::from_strided_rgba_scaled(1, 1, 4, &[0; 4], 1, 0).is_err());
    }

    #[test]
    fn strided_rgba_rejects_stride_shorter_than_a_pixel_row() {
        let source = [0_u8; 16];
        assert!(OwnedFrame::from_strided_rgba(2, 2, 7, &source).is_err());
        assert!(OwnedFrame::from_strided_rgba_scaled(2, 2, 7, &source, 1, 1).is_err());
    }

    #[test]
    fn strided_rgba_rejects_overflow_and_short_buffers() {
        assert!(OwnedFrame::from_strided_rgba(usize::MAX, 1, usize::MAX, &[]).is_err());
        assert!(OwnedFrame::from_strided_rgba(1, usize::MAX, 4, &[]).is_err());
        assert!(
            OwnedFrame::from_strided_rgba_scaled(1, 1, 4, &[0; 4], usize::MAX, 2).is_err()
        );

        let short_source = [0_u8; 19];
        assert!(OwnedFrame::from_strided_rgba(2, 2, 12, &short_source).is_err());
        assert!(OwnedFrame::from_strided_rgba_scaled(2, 2, 12, &short_source, 1, 2).is_err());
    }

    #[test]
    fn strided_rgba_accepts_minimum_buffer_with_row_padding() {
        let source = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
        ];

        let packed = OwnedFrame::from_strided_rgba(2, 2, 12, &source).expect("valid padded frame");
        assert_eq!(packed.width, 2);
        assert_eq!(packed.height, 2);
        assert_eq!(
            packed.data,
            vec![1, 2, 3, 255, 5, 6, 7, 255, 13, 14, 15, 255, 17, 18, 19, 255]
        );

        let scaled = OwnedFrame::from_strided_rgba_scaled(2, 2, 12, &source, 1, 2)
            .expect("valid padded scaled frame");
        assert_eq!(scaled.data, vec![1, 2, 3, 255, 13, 14, 15, 255]);
    }

    #[test]
    fn localized_app_name_uses_portuguese_camera_word() {
        assert_eq!(
            localized_app_name_for_locale("pt_BR.UTF-8"),
            "Galaxy Book Câmera"
        );
    }

    #[test]
    fn localized_app_name_uses_first_language_candidate() {
        assert_eq!(
            app::localization::first_locale_candidate("C:pt_BR.UTF-8:en_US.UTF-8"),
            Some("pt_BR.UTF-8")
        );
    }

    #[test]
    fn localized_camera_word_supports_traditional_chinese() {
        assert_eq!(localized_camera_word_for_locale("zh_TW.UTF-8"), "相機");
    }

    #[test]
    fn normalize_countdown_seconds_accepts_snapshot_values() {
        assert_eq!(normalize_countdown_seconds(0), 0);
        assert_eq!(normalize_countdown_seconds(3), 3);
        assert_eq!(normalize_countdown_seconds(5), 5);
        assert_eq!(normalize_countdown_seconds(10), 10);
        assert_eq!(normalize_countdown_seconds(7), 0);
    }

    #[test]
    fn natural_preset_returns_to_neutral_baseline() {
        let mut config = CameraConfig::default();
        config.width = Some(1280);
        config.height = Some(720);
        config.brightness = 0.22;
        config.exposure_value = 0.42;
        config.contrast = 1.30;
        config.saturation = 1.45;
        config.temperature = -0.15;
        config.tint = 0.09;
        config.gamma = 1.08;
        config.sharpness = 1.25;

        config.apply_preset(Preset::Natural);

        assert_eq!(config.brightness, 0.0);
        assert_eq!(config.exposure_value, -0.04);
        assert_eq!(config.contrast, 1.04);
        assert_eq!(config.saturation, 1.05);
        assert_eq!(config.temperature, 0.04);
        assert_eq!(config.tint, 0.0);
        assert_eq!(config.gamma, 1.0);
        assert_eq!(config.sharpness, 1.0);
        assert_eq!(config.width, Some(1280));
        assert_eq!(config.height, Some(720));
    }

    #[test]
    fn preview_zoom_options_start_with_one_x() {
        let resolution_options = fallback_preview_resolution_options();
        let zoom_options = derive_preview_zoom_options_from(&resolution_options);
        assert_eq!(zoom_options.len(), 5);
        assert_eq!(zoom_options[0].label, "1x");
        assert_eq!(zoom_options[1].label, "2x");
        assert_eq!(zoom_options[2].label, "3x");
        assert_eq!(zoom_options[3].label, "5x");
        assert_eq!(zoom_options[4].label, "10x");
        assert!(zoom_options[0].factor >= 1.0);
    }

    #[test]
    fn ov02c10_simple_tuning_file_is_shipped_with_project() {
        let tuning_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(DEV_TUNING_PATH_RELATIVE);
        assert!(tuning_path.is_file(), "missing tuning file: {}", tuning_path.display());
        assert_eq!(
            Path::new(SYSTEM_TUNING_PATH),
            Path::new("/usr/share/libcamera/ipa/simple/ov02c10.yaml")
        );
        assert_eq!(
            Path::new(SYSTEM_TUNING_PATH).file_name(),
            Some(std::ffi::OsStr::new("ov02c10.yaml"))
        );
        assert!(include_str!("../packaging/fedora/galaxybook-camera.spec")
            .contains("%{_datadir}/libcamera/ipa/simple/ov02c10.yaml"));
    }

    #[test]
    fn softisp_mode_accepts_only_cpu_or_gpu() {
        assert_eq!(normalize_softisp_mode("cpu"), "cpu");
        assert_eq!(normalize_softisp_mode(" GPU "), "gpu");
        assert_eq!(normalize_softisp_mode("invalid"), "cpu");
        assert_eq!(normalize_softisp_mode("cpu\nINJECTED=1"), "cpu");
    }

    #[test]
    fn preferred_camera_uses_ov02c10_model_or_id() {
        let cameras = vec![
            ("front-generic".to_string(), "generic".to_string()),
            ("sensor-two".to_string(), "ov02c10".to_string()),
        ];
        assert_eq!(
            preferred_camera_id_from(&cameras).as_deref(),
            Some("sensor-two")
        );

        let cameras_without_model = vec![
            ("front-generic".to_string(), String::new()),
            ("platform-OV02C10:00".to_string(), String::new()),
        ];
        assert_eq!(
            preferred_camera_id_from(&cameras_without_model).as_deref(),
            Some("platform-OV02C10:00")
        );
    }

    #[test]
    fn range_max_is_included_and_sorted_before_discrete_sizes() {
        let sizes = sorted_resolution_dimensions(
            [(1280, 720), (1920, 1080), (1920, 1080)],
            Some((1920, 1092)),
        );
        assert_eq!(sizes, vec![(1920, 1092), (1920, 1080), (1280, 720)]);
    }

    #[test]
    fn hardware_smoke_requires_at_least_one_camera() {
        assert!(require_camera_count(0).is_err());
        assert!(require_camera_count(1).is_ok());
    }

    #[test]
    fn smoke_artifact_directory_is_unique_and_removed_on_drop() {
        let directory = {
            let artifacts = SmokeArtifacts::create("unit-test").expect("temporary artifacts");
            assert!(artifacts.directory.is_dir());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                let mode = fs::metadata(&artifacts.directory)
                    .expect("temporary directory metadata")
                    .permissions()
                    .mode();
                assert_eq!(mode & 0o777, 0o700);
            }
            artifacts.directory.clone()
        };
        assert!(!directory.exists());
    }
}
