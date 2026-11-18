use std::borrow::Cow;

use cosmic::{Element, iced::Length, widget};

use crate::theme::SPACE_MD;

pub fn toggle<'a, M>(
    name: Cow<'a, str>,
    value: bool,
    on_change: impl Fn(bool) -> M + 'a,
) -> Element<'a, M>
where
    M: Clone + 'static,
{
    widget::toggler(value)
        .label(name.into_owned())
        .width(Length::Fill)
        .spacing(SPACE_MD)
        .on_toggle(on_change)
        .into()
}
