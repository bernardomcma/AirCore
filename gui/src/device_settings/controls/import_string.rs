use std::borrow::Cow;

use cosmic::{
    Element,
    iced::{Length, alignment},
    widget,
};

use crate::{
    components,
    device_settings::labeled_setting_row,
    fl,
    theme::{self, SPACE_SM},
};

pub fn import_string<'a, M>(
    name: Cow<'a, str>,
    text: Cow<'a, str>,
    on_input: impl Fn(String) -> M + 'static,
    on_submit: impl Fn(String) -> M + Clone + 'static,
    is_narrow: bool,
) -> Element<'a, M>
where
    M: Clone + 'static,
{
    let submit_text = text.to_string();
    let on_button_press = on_submit.clone();
    labeled_setting_row(
        name,
        widget::row![
            widget::text_input("", text)
                .style(theme::apple_text_input())
                .width(Length::Fill)
                .on_input(on_input)
                .on_submit(on_submit),
            components::secondary_button(fl!("import"), on_button_press(submit_text)),
        ]
        .spacing(SPACE_SM)
        .align_y(alignment::Vertical::Center)
        .width(Length::Fill),
        is_narrow,
    )
}
