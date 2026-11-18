use std::{cell::Cell, marker::PhantomData};

use cosmic::{
    Element,
    iced::{Color, Length, Point},
    widget::{self, canvas},
};
use itertools::Itertools;

use crate::{
    device_settings::eq_helpers::{normalize_eq_points, zero_db_y},
    theme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqualizerLineVariant {
    Hero,
    Thumbnail { active: bool },
}

#[derive(Debug)]
pub struct EqualizerLine<Message> {
    _message: PhantomData<Message>,
    cache: canvas::Cache,
    cached_is_dark: Cell<Option<bool>>,
    min: i16,
    max: i16,
    values: Vec<i16>,
    variant: EqualizerLineVariant,
}

impl<Message> EqualizerLine<Message> {
    #[must_use]
    pub fn new(min: i16, max: i16, values: Vec<i16>) -> Self {
        Self::with_variant(min, max, values, EqualizerLineVariant::Hero)
    }

    #[must_use]
    pub fn thumbnail(min: i16, max: i16, values: Vec<i16>, active: bool) -> Self {
        Self::with_variant(min, max, values, EqualizerLineVariant::Thumbnail { active })
    }

    #[must_use]
    pub fn with_variant(
        min: i16,
        max: i16,
        values: Vec<i16>,
        variant: EqualizerLineVariant,
    ) -> Self {
        Self {
            _message: PhantomData,
            cache: Default::default(),
            cached_is_dark: Cell::new(None),
            min,
            max,
            values,
            variant,
        }
    }

    fn sync_theme_cache(&self, theme: &cosmic::Theme) {
        let is_dark = theme.cosmic().is_dark;
        if self.cached_is_dark.get() != Some(is_dark) {
            self.cache.clear();
            self.cached_is_dark.set(Some(is_dark));
        }
    }

    pub fn set_values(&mut self, min: i16, max: i16, values: Vec<i16>) {
        if self.min != min || self.max != max || self.values != values {
            self.min = min;
            self.max = max;
            self.values = values;
            self.cache.clear();
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        widget::canvas(self)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

impl<Message> canvas::Program<Message, cosmic::Theme, cosmic::Renderer> for EqualizerLine<Message> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &cosmic::Renderer,
        theme: &cosmic::Theme,
        bounds: cosmic::iced::Rectangle,
        _cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Vec<canvas::Geometry<cosmic::iced::Renderer>> {
        self.sync_theme_cache(theme);
        let equalizer_line = self.cache.draw(renderer, bounds.size(), |frame| {
            let p = theme::palette(theme);
            let padding = match self.variant {
                EqualizerLineVariant::Hero => 10.0,
                EqualizerLineVariant::Thumbnail { .. } => 4.0,
            };

            let zero_y = zero_db_y(self.min, self.max, frame.height(), padding);

            match self.variant {
                EqualizerLineVariant::Hero => {
                    for y in [padding, zero_y, (frame.height() - padding).max(padding)] {
                        let is_zero = (y - zero_y).abs() < 0.5;
                        let color = if is_zero {
                            Color {
                                a: 0.35,
                                ..p.text_tertiary
                            }
                        } else {
                            Color {
                                a: 0.75,
                                ..p.border
                            }
                        };
                        frame.stroke(
                            &canvas::Path::line(Point::new(0.0, y), Point::new(frame.width(), y)),
                            canvas::Stroke {
                                style: canvas::stroke::Style::Solid(color),
                                width: 1.0,
                                line_cap: canvas::LineCap::Square,
                                line_join: canvas::LineJoin::Bevel,
                                line_dash: canvas::LineDash::default(),
                            },
                        );
                    }
                }
                EqualizerLineVariant::Thumbnail { active } => {
                    let line_color = if active {
                        Color {
                            a: 0.28,
                            ..p.accent
                        }
                    } else {
                        Color { a: 0.8, ..p.border }
                    };
                    frame.stroke(
                        &canvas::Path::line(
                            Point::new(0.0, zero_y),
                            Point::new(frame.width(), zero_y),
                        ),
                        canvas::Stroke {
                            style: canvas::stroke::Style::Solid(line_color),
                            width: 1.0,
                            line_cap: canvas::LineCap::Square,
                            line_join: canvas::LineJoin::Bevel,
                            line_dash: canvas::LineDash::default(),
                        },
                    );
                }
            }

            let raw_points = normalize_eq_points(
                &self.values,
                self.min,
                self.max,
                frame.width(),
                frame.height(),
                padding,
            );
            if raw_points.is_empty() {
                return;
            }
            let points: Vec<Point<f32>> = raw_points
                .into_iter()
                .map(|(x, y)| Point::new(x, y))
                .collect();

            let stroke_color: Color = match self.variant {
                EqualizerLineVariant::Hero => p.accent_text,
                EqualizerLineVariant::Thumbnail { active: true } => p.accent_text,
                EqualizerLineVariant::Thumbnail { active: false } => p.text_secondary,
            };

            if matches!(self.variant, EqualizerLineVariant::Hero) && points.len() >= 2 {
                let area = canvas::Path::new(|builder| {
                    builder.move_to(Point::new(points[0].x, zero_y));
                    for pt in &points {
                        builder.line_to(*pt);
                    }
                    if let Some(last) = points.last() {
                        builder.line_to(Point::new(last.x, zero_y));
                    }
                    builder.close();
                });
                let fill_alpha = if p.is_dark { 0.16 } else { 0.11 };
                frame.fill(
                    &area,
                    Color {
                        a: fill_alpha,
                        ..stroke_color
                    },
                );
            }

            let stroke_width = match self.variant {
                EqualizerLineVariant::Hero => 2.5,
                EqualizerLineVariant::Thumbnail { active: true } => 2.25,
                EqualizerLineVariant::Thumbnail { active: false } => 1.75,
            };

            for (left, right) in points.iter().copied().tuple_windows() {
                frame.stroke(
                    &canvas::Path::line(left, right),
                    canvas::Stroke {
                        style: canvas::stroke::Style::Solid(stroke_color),
                        width: stroke_width,
                        line_cap: canvas::LineCap::Round,
                        line_join: canvas::LineJoin::Round,
                        line_dash: canvas::LineDash::default(),
                    },
                );
            }

            if matches!(self.variant, EqualizerLineVariant::Hero) {
                for pt in points {
                    frame.fill(&canvas::Path::circle(pt, 3.5), stroke_color);
                    frame.fill(&canvas::Path::circle(pt, 1.5), p.card);
                }
            }
        });
        vec![equalizer_line]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_cache_tracks_light_and_dark_transitions() {
        let line: EqualizerLine<()> = EqualizerLine::new(-60, 60, vec![0; 8]);
        assert_eq!(line.cached_is_dark.get(), None);

        line.sync_theme_cache(&cosmic::Theme::light());
        assert_eq!(line.cached_is_dark.get(), Some(false));

        line.sync_theme_cache(&cosmic::Theme::dark());
        assert_eq!(line.cached_is_dark.get(), Some(true));

        line.sync_theme_cache(&cosmic::Theme::light());
        assert_eq!(line.cached_is_dark.get(), Some(false));
    }
}
