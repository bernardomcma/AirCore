use std::borrow::Cow;

use openscq30_lib::settings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetPreviewData {
    pub option: Cow<'static, str>,
    pub localized_name: String,
    pub values: Vec<i16>,
    pub is_active: bool,
}

#[must_use]
#[allow(dead_code)]
pub fn normalize_eq_points(
    values: &[i16],
    min: i16,
    max: i16,
    width: f32,
    height: f32,
    padding: f32,
) -> Vec<(f32, f32)> {
    if values.is_empty() {
        return Vec::new();
    }
    let usable_w = (width - padding * 2.0).max(1.0);
    let usable_h = (height - padding * 2.0).max(1.0);
    let range = i32::from(max) - i32::from(min);

    values
        .iter()
        .enumerate()
        .map(|(i, &raw)| {
            let norm_x = if values.len() == 1 {
                0.5
            } else {
                i as f32 / (values.len() - 1) as f32
            };
            let x = padding + norm_x * usable_w;
            let norm_y = if range <= 0 {
                0.5
            } else {
                let clamped = i32::from(raw).clamp(i32::from(min), i32::from(max));
                1.0 - ((clamped - i32::from(min)) as f32 / range as f32)
            };
            let y = padding + norm_y * usable_h;
            (x, y)
        })
        .collect()
}

#[must_use]
pub fn zero_db_y(min: i16, max: i16, height: f32, padding: f32) -> f32 {
    let usable_h = (height - padding * 2.0).max(1.0);
    let range = i32::from(max) - i32::from(min);
    if range <= 0 {
        return padding + usable_h * 0.5;
    }
    let clamped_zero = 0i32.clamp(i32::from(min), i32::from(max));
    let norm_y = 1.0 - ((clamped_zero - i32::from(min)) as f32 / range as f32);
    padding + norm_y * usable_h
}

#[must_use]
pub fn format_db_value(raw: i16, fraction_digits: i16) -> String {
    if fraction_digits <= 0 {
        return format!("{raw}");
    }
    let divisor = 10i32.pow(fraction_digits as u32);
    let raw_i32 = i32::from(raw);
    let sign = if raw_i32 < 0 { "-" } else { "" };
    let abs_val = raw_i32.abs();
    let integer_part = abs_val / divisor;
    let frac_part = abs_val % divisor;
    let width = fraction_digits as usize;
    format!("{sign}{integer_part}.{frac_part:0width$}")
}

#[must_use]
pub fn format_signed_db_value(raw: i16, fraction_digits: i16) -> String {
    let base = format_db_value(raw, fraction_digits);
    if raw > 0 { format!("+{base}") } else { base }
}

#[must_use]
pub fn format_db_list(values: &[i16], fraction_digits: i16) -> String {
    values
        .iter()
        .map(|&v| format_signed_db_value(v, fraction_digits))
        .collect::<Vec<_>>()
        .join(" · ")
}

#[must_use]
pub fn format_hz_label(hz: u16) -> String {
    if hz >= 1000 {
        let whole = hz / 1000;
        let remainder = (hz % 1000) / 100;
        if remainder == 0 {
            format!("{whole} kHz")
        } else {
            format!("{whole}.{remainder} kHz")
        }
    } else {
        format!("{hz} Hz")
    }
}

#[must_use]
pub fn build_preset_previews(
    equalizer: &settings::Equalizer,
    select: &settings::Select,
    presets: &[Vec<i16>],
    active_value: Option<&str>,
) -> Vec<PresetPreviewData> {
    let fallback_len = equalizer.band_hz.len();
    select
        .options
        .iter()
        .zip(select.localized_options.iter())
        .enumerate()
        .map(|(index, (option, localized_name))| {
            let values = presets
                .get(index)
                .cloned()
                .unwrap_or_else(|| vec![0; fallback_len]);
            let is_active = active_value.is_some_and(|active| active == option.as_ref());
            PresetPreviewData {
                option: option.clone(),
                localized_name: localized_name.clone(),
                values,
                is_active,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_eq_points_handles_empty_single_flat_and_clamped_ranges() {
        assert!(normalize_eq_points(&[], -60, 60, 100.0, 50.0, 4.0).is_empty());

        let single = normalize_eq_points(&[0], -60, 60, 100.0, 50.0, 4.0);
        assert_eq!(single.len(), 1);
        assert!((single[0].0 - 50.0).abs() < 0.01);
        assert!((single[0].1 - 25.0).abs() < 0.01);

        let degenerate = normalize_eq_points(&[10, -10], 0, 0, 100.0, 50.0, 4.0);
        assert!((degenerate[0].1 - 25.0).abs() < 0.01);
        assert!((degenerate[1].1 - 25.0).abs() < 0.01);

        let clamped = normalize_eq_points(&[-200, 0, 200], -60, 60, 108.0, 58.0, 4.0);
        assert!((clamped[0].1 - 54.0).abs() < 0.01);
        assert!((clamped[1].1 - 29.0).abs() < 0.01);
        assert!((clamped[2].1 - 4.0).abs() < 0.01);
    }

    #[test]
    fn format_db_and_hz_labels_format_cleanly() {
        assert_eq!(format_db_value(0, 1), "0.0");
        assert_eq!(format_db_value(-5, 1), "-0.5");
        assert_eq!(format_db_value(55, 1), "5.5");
        assert_eq!(format_db_value(-120, 1), "-12.0");
        assert_eq!(format_db_value(12, 0), "12");
        assert_eq!(format_signed_db_value(55, 1), "+5.5");
        assert_signed_zero_and_neg();

        assert_eq!(format_hz_label(100), "100 Hz");
        assert_eq!(format_hz_label(800), "800 Hz");
        assert_eq!(format_hz_label(1600), "1.6 kHz");
        assert_eq!(format_hz_label(2000), "2 kHz");
        assert_eq!(format_hz_label(12800), "12.8 kHz");
    }

    fn assert_signed_zero_and_neg() {
        assert_eq!(format_signed_db_value(0, 1), "0.0");
        assert_eq!(format_signed_db_value(-5, 1), "-0.5");
    }

    #[test]
    fn build_preset_previews_maps_options_and_active_flag_safely() {
        let equalizer = settings::Equalizer {
            band_hz: Cow::Borrowed(&[100, 200, 400, 800, 1600, 3200, 6400, 12800]),
            min: -60,
            max: 60,
            fraction_digits: 1,
        };
        let select = settings::Select {
            options: vec![
                Cow::Borrowed("SoundcoreSignature"),
                Cow::Borrowed("HardTechno"),
            ],
            localized_options: vec!["Soundcore Signature".to_string(), "Hard Techno".to_string()],
        };
        let presets = vec![
            vec![0, 0, 0, 0, 0, 0, 0, 0],
            vec![50, 40, 20, -10, 0, 25, 45, 35],
        ];
        let previews = build_preset_previews(&equalizer, &select, &presets, Some("HardTechno"));
        assert_eq!(previews.len(), 2);
        assert!(!previews[0].is_active);
        assert!(previews[1].is_active);
        assert_eq!(previews[1].localized_name, "Hard Techno");
        assert_eq!(previews[1].values, vec![50, 40, 20, -10, 0, 25, 45, 35]);
    }
}
