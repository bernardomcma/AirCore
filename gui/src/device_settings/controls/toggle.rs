use std::borrow::Cow;

use cosmic::{
    Element,
    iced::{Length, alignment, core::text::Wrapping},
    widget,
};

use crate::{fluid, theme::SPACE_MD};

pub fn toggle<'a, M>(
    name: Cow<'a, str>,
    value: bool,
    on_change: impl Fn(bool) -> M + 'a,
) -> Element<'a, M>
where
    M: Clone + 'static,
{
    let label = widget::text::body(name)
        .wrapping(Wrapping::Word)
        .width(Length::Fill);

    widget::row![label, fluid::fluid_switch(value, on_change)]
        .spacing(SPACE_MD)
        .align_y(alignment::Vertical::Center)
        .width(Length::Fill)
        .into()
}
