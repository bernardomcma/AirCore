use std::borrow::Cow;

use cosmic::{Element, iced::alignment, widget};

use crate::{
    device_settings::labeled_setting_row,
    fl,
    theme::{self, SPACE_XS},
};

pub struct TimeOfDayValues<'a> {
    pub maybe_is_pm: Option<bool>,
    pub hour_text: Cow<'a, str>,
    pub minute_text: Cow<'a, str>,
}

pub fn time_of_day<'a, M>(
    name: Cow<'a, str>,
    values: TimeOfDayValues<'a>,
    on_hour_change: impl Fn(String) -> M + Clone + 'static,
    on_minute_change: impl Fn(String) -> M + Clone + 'static,
    on_pm_change: impl Fn(bool) -> M + Clone + Send + Sync + 'static,
    on_refresh: M,
    is_narrow: bool,
) -> Element<'a, M>
where
    M: Clone + 'static,
{
    labeled_setting_row(
        name,
        widget::row![
            widget::inline_input(fl!("hour"), values.hour_text)
                .style(theme::apple_text_input())
                .width(50)
                .on_input(on_hour_change)
                .on_submit({
                    let on_refresh = on_refresh.clone();
                    move |_| on_refresh.clone()
                }),
            widget::text(":"),
            widget::inline_input(fl!("minute"), values.minute_text)
                .style(theme::apple_text_input())
                .width(50)
                .on_input(on_minute_change)
                .on_submit(move |_| on_refresh.clone()),
            values.maybe_is_pm.map(|is_pm| {
                widget::dropdown(
                    vec![fl!("am"), fl!("pm")],
                    Some(usize::from(is_pm)),
                    move |index| on_pm_change(index == 1),
                )
            }),
        ]
        .spacing(SPACE_XS)
        .align_y(alignment::Vertical::Center),
        is_narrow,
    )
}
