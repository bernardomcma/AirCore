use std::borrow::Cow;

use cosmic::Element;

use crate::{components, device_settings::labeled_setting_row, fl};

pub fn action<M>(name: Cow<'_, str>, on_execute: M, is_narrow: bool) -> Element<'_, M>
where
    M: Clone + 'static,
{
    labeled_setting_row(
        name,
        components::secondary_button(fl!("execute"), on_execute),
        is_narrow,
    )
}
