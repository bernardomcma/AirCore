use std::borrow::Cow;

use cosmic::{
    Element,
    iced::{Length, alignment},
    widget,
};
use openscq30_lib::settings::Select;

use crate::{
    device_settings::{ios_hero, labeled_setting_row},
    fl,
    theme::{SPACE_MD, SPACE_SM, SPACE_XS},
};

pub fn select<'a, M>(
    name: Cow<'a, str>,
    setting: &'a Select,
    value: &str,
    on_change: impl Fn(&str) -> M + Send + Sync + 'static,
    is_narrow: bool,
) -> Element<'a, M>
where
    M: Clone + 'static,
{
    if (2..=4).contains(&setting.options.len()) {
        return labeled_setting_row(
            name,
            ios_hero::ios_segmented_control(None, setting, value, false, on_change),
            is_narrow,
        );
    }

    let selected_index = setting.options.iter().position(|option| option == value);
    let options = setting.options.to_owned();
    labeled_setting_row(
        name,
        widget::dropdown(&setting.localized_options, selected_index, move |index| {
            on_change(&options[index])
        })
        .width(Length::Fill),
        is_narrow,
    )
}

pub fn optional_select<'a, M>(
    name: Cow<'a, str>,
    setting: &'a Select,
    value: Option<&str>,
    on_change: impl Fn(Option<&str>) -> M + Send + Sync + 'static,
    is_narrow: bool,
) -> Element<'a, M>
where
    M: Clone + 'static,
{
    let localized_items_with_none_option = [fl!("none")]
        .into_iter()
        .chain(setting.localized_options.to_owned())
        .collect::<Vec<_>>();
    let selected_index = value
        .and_then(|value| {
            setting
                .options
                .iter()
                .position(|option| option == value)
                .map(|pos| pos + 1)
        })
        .unwrap_or_default();
    let options = setting.options.to_owned();
    labeled_setting_row(
        name,
        widget::row![
            widget::dropdown(
                localized_items_with_none_option,
                Some(selected_index),
                move |index| {
                    on_change(if index == 0 {
                        None
                    } else {
                        Some(&options[index - 1])
                    })
                },
            )
            .width(Length::Fill),
        ],
        is_narrow,
    )
}

pub fn modifiable_select<'a, M>(
    name: Cow<'a, str>,
    setting: &'a Select,
    value: Option<&str>,
    on_change: impl Fn(&str) -> M + Send + Sync + 'static,
    on_add: M,
    on_remove: M,
    is_narrow: bool,
) -> Element<'a, M>
where
    M: Clone + 'static,
{
    let selected_index =
        value.and_then(|value| setting.options.iter().position(|option| option == value));
    let maybe_deselect_message = value.is_some().then_some(on_remove);
    let options = setting.options.to_owned();
    let add_label = fl!("add-item", name = name.as_ref());
    let remove_label = fl!("remove-item", name = name.as_ref());

    labeled_setting_row(
        name,
        widget::Row::with_capacity(3)
            .spacing(SPACE_XS)
            .align_y(alignment::Vertical::Center)
            .push(
                widget::dropdown(&setting.localized_options, selected_index, move |index| {
                    on_change(&options[index])
                })
                .width(Length::Fill),
            )
            .push_maybe(maybe_deselect_message.map(|deselect_message| {
                widget::button::icon(widget::icon::from_name("list-remove-symbolic"))
                    .name(remove_label.clone())
                    .tooltip(remove_label)
                    .on_press(deselect_message)
            }))
            .push(
                widget::button::icon(widget::icon::from_name("list-add-symbolic"))
                    .name(add_label.clone())
                    .tooltip(add_label)
                    .on_press(on_add),
            ),
        is_narrow,
    )
}

pub fn multi_select<'a, M>(
    _name: Cow<'a, str>,
    setting: &'a Select,
    values: &'a [Cow<'static, str>],
    on_change: impl Fn(Vec<Cow<'static, str>>) -> M + Send + Sync + Clone + 'static,
) -> Vec<Element<'a, M>>
where
    M: Clone + 'static,
{
    setting
        .options
        .iter()
        .zip(setting.localized_options.iter())
        .map(|(option, localized_option)| {
            let option = option.clone();
            let values = values.to_vec();
            let on_change = on_change.clone();
            widget::toggler(values.contains(&option))
                .label(localized_option.to_owned())
                .width(Length::Fill)
                .spacing(SPACE_MD)
                .on_toggle(move |is_checked| {
                    if !is_checked {
                        on_change(values.iter().filter(|o| **o != option).cloned().collect())
                    } else {
                        on_change(values.iter().chain([&option]).cloned().collect())
                    }
                })
                .into()
        })
        .collect()
}

pub fn multi_select_with_remove<'a, M>(
    _name: Cow<'a, str>,
    setting: &'a Select,
    values: &'a [Cow<'static, str>],
    on_change: impl Fn(Vec<Cow<'static, str>>) -> M + Send + Sync + Clone + 'static,
    on_remove: impl Fn(Cow<'static, str>) -> M,
) -> Vec<Element<'a, M>>
where
    M: Clone + 'static,
{
    setting
        .options
        .iter()
        .zip(setting.localized_options.iter())
        .map(|(option, localized_option)| {
            let option = option.clone();
            let values = values.to_owned();
            let is_checked = values.contains(&option);
            let on_change = on_change.clone();

            let remove_message = on_remove(option.clone());
            let on_toggle = move |is_checked: bool| {
                if !is_checked {
                    on_change(values.iter().filter(|o| **o != option).cloned().collect())
                } else {
                    on_change(
                        values
                            .iter()
                            .chain(std::iter::once(&option))
                            .cloned()
                            .collect(),
                    )
                }
            };

            widget::row::with_capacity(2)
                .spacing(SPACE_SM)
                .align_y(alignment::Vertical::Center)
                .push(
                    widget::toggler(is_checked)
                        .label(localized_option.to_owned())
                        .width(Length::Fill)
                        .spacing(SPACE_MD)
                        .on_toggle(on_toggle),
                )
                .push_maybe((!is_checked).then(|| {
                    widget::button::icon(widget::icon::from_name("list-remove-symbolic"))
                        .class(cosmic::theme::Button::Destructive)
                        .name(fl!("remove"))
                        .tooltip(fl!("remove"))
                        .on_press(remove_message)
                }))
                .width(Length::Fill)
                .into()
        })
        .collect()
}
