use std::borrow::Cow;

use cosmic::{
    Element,
    iced::{Length, alignment, core::text::Wrapping},
    widget,
};

use crate::{
    device_settings::labeled_setting_row,
    fl,
    theme::{self, SPACE_SM},
};

pub fn information<'a, M>(
    name: Cow<'a, str>,
    text: Cow<'a, str>,
    on_copy: M,
    is_narrow: bool,
) -> Element<'a, M>
where
    M: Clone + 'static,
{
    let copy_label = format!("{} {}", fl!("copy"), name);
    labeled_setting_row(
        name,
        widget::row![
            theme::mono_value(text)
                .wrapping(Wrapping::WordOrGlyph)
                .width(Length::Fill),
            widget::button::icon(widget::icon::from_name("edit-copy-symbolic"))
                .name(copy_label)
                .tooltip(fl!("copy"))
                .on_press(on_copy),
        ]
        .spacing(SPACE_SM)
        .align_y(alignment::Vertical::Center)
        .width(Length::Fill),
        is_narrow,
    )
}
