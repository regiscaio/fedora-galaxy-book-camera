use crate::{CameraConfig, OwnedFrame};

#[derive(Clone)]
pub(crate) struct AdjustmentProfile {
    red_lut: [u8; 256],
    green_lut: [u8; 256],
    blue_lut: [u8; 256],
    has_lut_adjustments: bool,
    has_brightness_contrast_adjustments: bool,
    has_hue_saturation_adjustments: bool,
    has_extreme_luma_neutralization: bool,
    brightness_offset: f32,
    contrast: f32,
    saturation: f32,
    hue_sin: f32,
    hue_cos: f32,
    sharpen_amount: f32,
    mirror: bool,
}

impl AdjustmentProfile {
    pub(crate) fn new(config: &CameraConfig) -> Self {
        let has_lut_adjustments = config.exposure_value.abs() > 0.001
            || (config.gamma - 1.0).abs() > 0.001
            || config.temperature.abs() > 0.001
            || config.tint.abs() > 0.001
            || (config.red_gain - 1.0).abs() > 0.001
            || (config.green_gain - 1.0).abs() > 0.001
            || (config.blue_gain - 1.0).abs() > 0.001;

        let temperature = config.temperature as f32;
        let tint = config.tint as f32;
        let brightness_offset = config.brightness as f32;
        let contrast = (config.contrast as f32).max(0.0);
        let saturation = (config.saturation as f32).max(0.0);
        let hue_angle = (config.hue as f32) * std::f32::consts::PI;
        let exposure_scale = 2.0_f32.powf((config.exposure_value as f32) * 0.6);
        let gamma_inverse = 1.0_f32 / (config.gamma as f32).max(0.05);
        let red_scale = (config.red_gain as f32) * (1.0 + 0.25 * temperature) * (1.0 + 0.08 * tint);
        let green_scale = (config.green_gain as f32) * (1.0 - 0.20 * tint);
        let blue_scale = (config.blue_gain as f32) * (1.0 - 0.25 * temperature) * (1.0 + 0.08 * tint);

        let mut red_lut = [0_u8; 256];
        let mut green_lut = [0_u8; 256];
        let mut blue_lut = [0_u8; 256];

        for value in 0..=255 {
            red_lut[value as usize] =
                adjust_color_channel(value as u8, red_scale, exposure_scale, gamma_inverse);
            green_lut[value as usize] =
                adjust_color_channel(value as u8, green_scale, exposure_scale, gamma_inverse);
            blue_lut[value as usize] =
                adjust_color_channel(value as u8, blue_scale, exposure_scale, gamma_inverse);
        }

        Self {
            red_lut,
            green_lut,
            blue_lut,
            has_lut_adjustments,
            has_brightness_contrast_adjustments: brightness_offset.abs() > 0.001
                || (contrast - 1.0).abs() > 0.001,
            has_hue_saturation_adjustments: (saturation - 1.0).abs() > 0.001
                || hue_angle.abs() > 0.001,
            has_extreme_luma_neutralization: true,
            brightness_offset,
            contrast,
            saturation,
            hue_sin: hue_angle.sin(),
            hue_cos: hue_angle.cos(),
            sharpen_amount: (config.sharpness - 1.0).max(0.0) as f32,
            mirror: config.mirror,
        }
    }
}

fn adjust_color_channel(
    channel: u8,
    channel_scale: f32,
    exposure_scale: f32,
    gamma_inverse: f32,
) -> u8 {
    let scaled = ((channel as f32) / 255.0 * channel_scale * exposure_scale).clamp(0.0, 1.0);
    let corrected = scaled.powf(gamma_inverse);
    (corrected * 255.0).clamp(0.0, 255.0).round() as u8
}

pub(crate) fn apply_adjustments(frame: &mut OwnedFrame, profile: &AdjustmentProfile) {
    if profile.has_lut_adjustments
        || profile.has_brightness_contrast_adjustments
        || profile.has_hue_saturation_adjustments
        || profile.has_extreme_luma_neutralization
    {
        for pixel in frame.data.chunks_exact_mut(4) {
            let mut red = if profile.has_lut_adjustments {
                profile.red_lut[pixel[0] as usize]
            } else {
                pixel[0]
            };
            let mut green = if profile.has_lut_adjustments {
                profile.green_lut[pixel[1] as usize]
            } else {
                pixel[1]
            };
            let mut blue = if profile.has_lut_adjustments {
                profile.blue_lut[pixel[2] as usize]
            } else {
                pixel[2]
            };

            if profile.has_hue_saturation_adjustments {
                let red_f = red as f32 / 255.0;
                let green_f = green as f32 / 255.0;
                let blue_f = blue as f32 / 255.0;

                let luma = 0.299 * red_f + 0.587 * green_f + 0.114 * blue_f;
                let chroma_i = 0.596 * red_f - 0.274 * green_f - 0.322 * blue_f;
                let chroma_q = 0.211 * red_f - 0.523 * green_f + 0.312 * blue_f;

                let saturated_i = chroma_i * profile.saturation;
                let saturated_q = chroma_q * profile.saturation;
                let rotated_i = saturated_i * profile.hue_cos - saturated_q * profile.hue_sin;
                let rotated_q = saturated_i * profile.hue_sin + saturated_q * profile.hue_cos;

                red = (luma + 0.956 * rotated_i + 0.621 * rotated_q)
                    .mul_add(255.0, 0.0)
                    .clamp(0.0, 255.0)
                    .round() as u8;
                green = (luma - 0.272 * rotated_i - 0.647 * rotated_q)
                    .mul_add(255.0, 0.0)
                    .clamp(0.0, 255.0)
                    .round() as u8;
                blue = (luma - 1.106 * rotated_i + 1.703 * rotated_q)
                    .mul_add(255.0, 0.0)
                    .clamp(0.0, 255.0)
                    .round() as u8;
            }

            if profile.has_brightness_contrast_adjustments {
                red =
                    adjust_brightness_contrast(red, profile.brightness_offset, profile.contrast);
                green = adjust_brightness_contrast(
                    green,
                    profile.brightness_offset,
                    profile.contrast,
                );
                blue =
                    adjust_brightness_contrast(blue, profile.brightness_offset, profile.contrast);
            }

            if profile.has_extreme_luma_neutralization {
                (red, green, blue) = neutralize_extreme_luma_casts(red, green, blue);
            }

            pixel[0] = red;
            pixel[1] = green;
            pixel[2] = blue;
        }
    }

    if profile.sharpen_amount > 0.001 {
        apply_sharpen(frame, profile.sharpen_amount);
    }

    if profile.mirror {
        mirror_frame_horizontal(frame);
    }
}

fn adjust_brightness_contrast(channel: u8, brightness_offset: f32, contrast: f32) -> u8 {
    let normalized = channel as f32 / 255.0;
    let adjusted = ((normalized - 0.5) * contrast + 0.5 + brightness_offset).clamp(0.0, 1.0);
    (adjusted * 255.0).round() as u8
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    if edge0 == edge1 {
        return if value < edge0 { 0.0 } else { 1.0 };
    }

    let normalized = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    normalized * normalized * (3.0 - 2.0 * normalized)
}

fn neutralize_extreme_luma_casts(red: u8, green: u8, blue: u8) -> (u8, u8, u8) {
    let red_f = red as f32 / 255.0;
    let green_f = green as f32 / 255.0;
    let blue_f = blue as f32 / 255.0;

    let luma = 0.299 * red_f + 0.587 * green_f + 0.114 * blue_f;
    let shadow_weight = 1.0 - smoothstep(0.03, 0.24, luma);
    let highlight_weight = smoothstep(0.74, 0.98, luma);
    let neutralize_amount = shadow_weight * 0.34 + highlight_weight * 0.12;

    if neutralize_amount <= 0.001 {
        return (red, green, blue);
    }

    let blend = |channel: f32| -> u8 {
        (channel + (luma - channel) * neutralize_amount)
            .mul_add(255.0, 0.0)
            .clamp(0.0, 255.0)
            .round() as u8
    };

    (blend(red_f), blend(green_f), blend(blue_f))
}

fn apply_sharpen(frame: &mut OwnedFrame, amount: f32) {
    let width = frame.width;
    let height = frame.height;
    if width < 3 || height < 3 {
        return;
    }

    let original = frame.data.clone();
    let stride = width * 4;
    let kernel_amount = amount.min(1.0) * 0.25;

    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let center = y * stride + x * 4;
            let left = center - 4;
            let right = center + 4;
            let up = center - stride;
            let down = center + stride;

            for channel in 0..3 {
                let center_value = original[center + channel] as f32;
                let neighbors = original[left + channel] as f32
                    + original[right + channel] as f32
                    + original[up + channel] as f32
                    + original[down + channel] as f32;
                let sharpened =
                    center_value * (1.0 + 4.0 * kernel_amount) - neighbors * kernel_amount;
                frame.data[center + channel] = sharpened.clamp(0.0, 255.0).round() as u8;
            }
        }
    }
}

fn mirror_frame_horizontal(frame: &mut OwnedFrame) {
    if frame.width < 2 {
        return;
    }

    let row_stride = frame.width * 4;
    for row in frame.data.chunks_exact_mut(row_stride) {
        for x in 0..(frame.width / 2) {
            let left = x * 4;
            let right = (frame.width - 1 - x) * 4;
            let (before_right, right_and_after) = row.split_at_mut(right);
            before_right[left..(left + 4)].swap_with_slice(&mut right_and_after[..4]);
        }
    }
}
