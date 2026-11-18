use std::borrow::Cow;

use cosmic::{
    Apply, Element, Renderer,
    iced::{Alignment, Length, core::text::Wrapping},
    theme,
    widget::{self, icon},
};

use crate::{
    fl,
    theme::{SPACE_LG, SPACE_MD, SPACE_SM, SemanticTone, alert_banner_container},
};

#[must_use]
pub fn warning<'a, Message>(message: impl Into<Cow<'a, str>>) -> Warning<'a, Message> {
    Warning {
        message: message.into(),
        on_close: None,
    }
}

pub struct Warning<'a, Message> {
    message: Cow<'a, str>,
    on_close: Option<Message>,
}

impl<'a, Message: 'static + Clone> Warning<'a, Message> {
    /// The message to emit on button press.
    #[must_use]
    pub fn on_close(mut self, message: Message) -> Self {
        self.on_close = Some(message);
        self
    }

    /// Renders an Apple-inspired semantic alert banner with AccessKit-labelled close button.
    pub fn into_widget(self) -> widget::Container<'a, Message, cosmic::Theme, Renderer> {
        let icon_badge = icon::from_name("dialog-warning-symbolic").size(16);
        let label = cosmic::widget::text::body(self.message)
            .wrapping(Wrapping::WordOrGlyph)
            .width(Length::Fill);

        let close_button = icon::from_name("window-close-symbolic")
            .size(16)
            .apply(widget::button::icon)
            .name(fl!("close"))
            .tooltip(fl!("close"))
            .on_press_maybe(self.on_close);

        widget::row::with_capacity(3)
            .push(icon_badge)
            .push(label)
            .push(close_button)
            .spacing(SPACE_SM)
            .align_y(Alignment::Center)
            .apply(widget::container)
            .class(theme::Container::custom(alert_banner_container(
                SemanticTone::Warning,
            )))
            .padding([SPACE_MD, SPACE_LG])
            .align_y(Alignment::Center)
            .width(Length::Fill)
    }
}

impl<'a, Message: 'static + Clone> From<Warning<'a, Message>> for Element<'a, Message> {
    fn from(warning: Warning<'a, Message>) -> Self {
        Self::from(warning.into_widget())
    }
}
