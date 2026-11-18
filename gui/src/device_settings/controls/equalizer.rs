use cosmic::{
    Element,
    iced::{Length, alignment},
    widget,
};
use openscq30_lib::settings::Equalizer;

use crate::{
    device_settings::eq_helpers::format_db_value,
    fl, fluid,
    theme::{self, SPACE_SM, SPACE_XS},
};

pub fn horizontal_equalizer<'a, M>(
    setting: &'a Equalizer,
    value: &'a [i16],
    on_change: impl Fn(u8, i16) -> M + 'static + Clone,
) -> Vec<Element<'a, M>>
where
    M: Clone + 'static,
{
    let midpoint = (setting.min + setting.max) / 2;
    setting
        .band_hz
        .iter()
        .copied()
        .enumerate()
        .map(move |(i, hz)| {
            let band_val = value.get(i).copied().unwrap_or(midpoint);
            let hz_label = fl!("hz", hz = hz);
            let db_text = format_db_value(band_val, setting.fraction_digits);

            widget::column![
                widget::row![
                    theme::row_title(hz_label.clone()).width(Length::Fill),
                    theme::mono_value(fl!("db", db = db_text)),
                ]
                .align_y(alignment::Vertical::Center),
                widget::row![
                    widget::spin_button(
                        format_db_value(band_val, setting.fraction_digits),
                        hz_label.clone(),
                        band_val,
                        1,
                        setting.min,
                        setting.max,
                        {
                            let on_change = on_change.clone();
                            move |band_value| on_change(i as u8, band_value)
                        },
                    ),
                    fluid::fluid_slider(setting.min..=setting.max, band_val, {
                        let on_change = on_change.clone();
                        move |band_value| on_change(i as u8, band_value)
                    }),
                ]
                .spacing(SPACE_SM)
                .align_y(alignment::Vertical::Center),
            ]
            .spacing(SPACE_XS)
            .width(Length::Fill)
            .into()
        })
        .collect()
}
