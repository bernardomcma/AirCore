use std::borrow::Cow;

use cosmic::{
    Element,
    cosmic_theme::palette::{Hsv, IntoColor, Srgb},
    iced::{Color, alignment},
    widget::{self, canvas},
};

use crate::{
    components,
    device_settings::labeled_setting_row,
    fl,
    theme::{self, SPACE_MD, SemanticTone},
};

pub fn hue_color_picker<'a, M>(
    name: Cow<'a, str>,
    hue: f32,
    on_change: impl Fn(f32) -> M + 'a,
    is_narrow: bool,
) -> Element<'a, M>
where
    M: Clone + 'static,
{
    labeled_setting_row(
        name.clone(),
        widget::row![
            widget::canvas(HueBox(hue)).width(24).height(24),
            widget::slider(0.0..=360.0, hue, on_change)
                .name(name)
                .class(theme::apple_slider())
                .description(fl!("color-hue-in-degrees")),
            components::pill_badge(format!("{hue:.0}°"), SemanticTone::Neutral),
        ]
        .align_y(alignment::Vertical::Center)
        .spacing(SPACE_MD),
        is_narrow,
    )
}

struct HueBox(f32);

impl<Message> canvas::Program<Message, cosmic::Theme, cosmic::Renderer> for HueBox {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &cosmic::Renderer,
        theme: &cosmic::Theme,
        bounds: cosmic::iced::Rectangle,
        _cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Vec<canvas::Geometry<cosmic::Renderer>> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let radius = (frame.width().min(frame.height()) / 2.0 - 1.5).max(1.0);
        let circle = canvas::Path::circle(frame.center(), radius);

        let hsv = Hsv::new(self.0, 1.0, 1.0);
        let rgb: Srgb = hsv.into_color();

        frame.fill(&circle, Color::from_rgb(rgb.red, rgb.green, rgb.blue));
        frame.stroke(
            &circle,
            canvas::Stroke {
                style: canvas::stroke::Style::Solid(theme::palette(theme).control_border),
                width: 1.0,
                line_cap: canvas::LineCap::Round,
                line_join: canvas::LineJoin::Round,
                line_dash: canvas::LineDash::default(),
            },
        );
        vec![frame.into_geometry()]
    }
}
