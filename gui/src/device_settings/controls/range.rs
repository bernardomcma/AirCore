use std::borrow::Cow;

use cosmic::{Element, iced::alignment, widget};
use openscq30_lib::settings;

use crate::{
    components,
    device_settings::labeled_setting_row,
    theme::{self, SPACE_MD, SemanticTone},
};

pub fn i32_range<'a, M>(
    name: Cow<'a, str>,
    range: settings::Range<i32>,
    value: i32,
    on_change: impl Fn(i32) -> M + 'a,
    is_narrow: bool,
) -> Element<'a, M>
where
    M: Clone + 'static,
{
    labeled_setting_row(
        name.clone(),
        widget::row![
            components::pill_badge(value.to_string(), SemanticTone::Neutral),
            widget::slider(range.range, value, on_change)
                .name(name)
                .step(range.step)
                .class(theme::apple_slider()),
        ]
        .spacing(SPACE_MD)
        .align_y(alignment::Vertical::Center),
        is_narrow,
    )
}
