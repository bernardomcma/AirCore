//! Reusable Apple-inspired layout and UI primitives built atop `crate::theme`.

use std::borrow::Cow;

use cosmic::{
    Apply, Element, Renderer, Theme,
    iced::{
        Alignment, Length,
        alignment::{Horizontal, Vertical},
        core::text::Wrapping,
    },
    widget,
};

use crate::{
    fluid,
    theme::{
        self, MAX_CONTENT_WIDTH, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS, SemanticTone,
    },
};

/// Wraps a scrollable/centered page body with consistent horizontal gutters and max width.
pub fn page_frame<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    widget::container(
        widget::container(content)
            .width(Length::Fill)
            .max_width(MAX_CONTENT_WIDTH),
    )
    .width(Length::Fill)
    .align_x(Horizontal::Center)
    .padding([SPACE_LG, SPACE_LG])
    .into()
}

/// Consistent page header with a clean noun title, optional muted subtitle, and optional trailing action.
pub fn page_header<'a, M: 'static>(
    title: impl Into<Cow<'a, str>> + 'a,
    subtitle: Option<Cow<'a, str>>,
    trailing: Option<Element<'a, M>>,
) -> Element<'a, M> {
    let title_block = widget::column![theme::page_title(title)]
        .push_maybe(subtitle.map(|sub| theme::row_subtitle(sub).wrapping(Wrapping::Word)))
        .spacing(SPACE_XS)
        .width(Length::Fill);

    widget::row![title_block]
        .push_maybe(trailing)
        .spacing(SPACE_MD)
        .align_y(Vertical::Center)
        .width(Length::Fill)
        .into()
}

/// Flat elevated surface card with 1px hairline border and standard padding.
pub fn elevated_card<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
) -> widget::Container<'a, M, Theme, Renderer> {
    widget::container(content)
        .class(cosmic::theme::Container::custom(theme::card_container))
        .padding(SPACE_LG)
        .width(Length::Fill)
}

/// Primary Apple Blue filled action button with explicit AccessKit name and tactile press squish.
pub fn primary_button<'a, M: Clone + 'static>(
    label: impl Into<Cow<'a, str>> + 'a,
    on_press: M,
) -> Element<'a, M> {
    let label_cow: Cow<'a, str> = label.into();
    let name_str = label_cow.to_string();
    fluid::fluid_pressable(
        widget::button::custom(widget::text::body(label_cow).align_x(Horizontal::Center))
            .name(name_str)
            .class(theme::primary_button())
            .padding([SPACE_SM, SPACE_LG])
            .on_press(on_press),
    )
}

/// Secondary neutral bordered action button with explicit AccessKit name and tactile press squish.
pub fn secondary_button<'a, M: Clone + 'static>(
    label: impl Into<Cow<'a, str>> + 'a,
    on_press: M,
) -> Element<'a, M> {
    let label_cow: Cow<'a, str> = label.into();
    let name_str = label_cow.to_string();
    fluid::fluid_pressable(
        widget::button::custom(widget::text::body(label_cow).align_x(Horizontal::Center))
            .name(name_str)
            .class(theme::secondary_button())
            .padding([SPACE_SM, SPACE_LG])
            .on_press(on_press),
    )
}

/// Destructive tinted/bordered action button with explicit AccessKit name and tactile press squish.
pub fn destructive_button<'a, M: Clone + 'static>(
    label: impl Into<Cow<'a, str>> + 'a,
    on_press: M,
) -> Element<'a, M> {
    let label_cow: Cow<'a, str> = label.into();
    let name_str = label_cow.to_string();
    fluid::fluid_pressable(
        widget::button::custom(widget::text::body(label_cow).align_x(Horizontal::Center))
            .name(name_str)
            .class(theme::destructive_button())
            .padding([SPACE_SM, SPACE_LG])
            .on_press(on_press),
    )
}

/// Apple System Settings-style inset grouped section: optional muted header above a card of rows separated by 1px hairlines.
pub fn inset_grouped_section<'a, M: 'static>(
    title: Option<Cow<'a, str>>,
    rows: Vec<Element<'a, M>>,
) -> Element<'a, M> {
    let mut card_children: Vec<Element<'a, M>> = Vec::with_capacity(rows.len().saturating_mul(2));
    for (idx, row) in rows.into_iter().enumerate() {
        if idx > 0 {
            card_children.push(
                cosmic::iced::widget::rule::horizontal(1)
                    .class(cosmic::theme::Rule::custom(theme::hairline_rule))
                    .into(),
            );
        }
        card_children.push(
            widget::container(row)
                .padding([SPACE_MD, SPACE_LG])
                .width(Length::Fill)
                .into(),
        );
    }

    let card = widget::container(widget::column(card_children).width(Length::Fill))
        .class(cosmic::theme::Container::custom(theme::card_container))
        .width(Length::Fill);

    widget::column![]
        .push_maybe(title.map(|t| {
            Element::from(widget::container(theme::section_title(t)).padding([0, SPACE_XS]))
        }))
        .push(card)
        .spacing(SPACE_SM)
        .width(Length::Fill)
        .into()
}

/// Consistent row layout inside grouped sections (title + optional subtitle on left, control on right).
pub fn setting_row<'a, M: 'static>(
    title: impl Into<Cow<'a, str>> + 'a,
    subtitle: Option<Cow<'a, str>>,
    control: impl Into<Element<'a, M>>,
    is_narrow: bool,
) -> Element<'a, M> {
    let label_block = widget::column![widget::text::body(title).wrapping(Wrapping::Word),]
        .push_maybe(subtitle.map(|sub| theme::row_subtitle(sub).wrapping(Wrapping::Word)))
        .spacing(2);

    if is_narrow {
        widget::column![
            label_block.width(Length::Fill),
            widget::container(control).width(Length::Fill),
        ]
        .spacing(SPACE_SM)
        .width(Length::Fill)
        .into()
    } else {
        widget::row![
            label_block.width(Length::FillPortion(2)),
            widget::container(control)
                .width(Length::FillPortion(3))
                .align_x(Horizontal::Right),
        ]
        .spacing(SPACE_LG)
        .align_y(Vertical::Center)
        .width(Length::Fill)
        .into()
    }
}

/// Compact pill status badge (e.g. DEMO MODE, degree readout, category tag).
pub fn pill_badge<'a, M: 'static>(
    label: impl Into<Cow<'a, str>> + 'a,
    tone: SemanticTone,
) -> Element<'a, M> {
    widget::container(widget::text::caption_heading(label))
        .class(cosmic::theme::Container::custom(
            theme::pill_badge_container(tone),
        ))
        .padding([3, 10])
        .into()
}

/// Clean centered empty state card using an embedded icon handle (zero missing-icon warnings).
pub fn empty_state<'a, M: 'static>(
    icon_handle: widget::icon::Handle,
    title: impl Into<Cow<'a, str>> + 'a,
    description: impl Into<Cow<'a, str>> + 'a,
    primary_action: Option<Element<'a, M>>,
) -> Element<'a, M> {
    widget::column![
        widget::icon(icon_handle).size(48),
        widget::text::title4(title).align_x(Horizontal::Center),
        theme::row_subtitle(description)
            .align_x(Horizontal::Center)
            .wrapping(Wrapping::Word),
    ]
    .push_maybe(primary_action)
    .spacing(SPACE_MD)
    .align_x(Alignment::Center)
    .max_width(440.0)
    .apply(widget::container)
    .padding([SPACE_XL, SPACE_LG])
    .center_x(Length::Fill)
    .apply(elevated_card)
    .into()
}
