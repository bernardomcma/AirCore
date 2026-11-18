use std::{cell::Cell, marker::PhantomData, time::Instant};

use cosmic::{
    Element,
    iced::{Color, Length, Point, window},
    widget::{self, canvas},
};

use crate::{
    device_settings::eq_helpers::zero_db_y,
    spring::{Spring, SpringParams},
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

pub struct EqualizerLineState {
    springs: Vec<Spring>,
    last_tick: Option<Instant>,
}

impl Default for EqualizerLineState {
    fn default() -> Self {
        Self {
            springs: Vec::new(),
            last_tick: None,
        }
    }
}

fn normalize_float_points(
    values: &[f32],
    min: i16,
    max: i16,
    width: f32,
    height: f32,
    padding: f32,
) -> Vec<Point<f32>> {
    if values.is_empty() {
        return Vec::new();
    }
    let min_f = f32::from(min);
    let max_f = f32::from(max);
    let span = (max_f - min_f).max(1.0);
    let usable_h = (height - padding * 2.0).max(1.0);
    let len = values.len();

    values
        .iter()
        .enumerate()
        .map(|(i, &val)| {
            let x = if len == 1 {
                width * 0.5
            } else {
                padding + (width - padding * 2.0) * (i as f32 / (len - 1) as f32)
            };
            let norm = ((val - min_f) / span).clamp(0.0, 1.0);
            let y = padding + usable_h * (1.0 - norm);
            Point::new(x, y)
        })
        .collect()
}

fn trace_smooth_curve(
    builder: &mut canvas::path::Builder,
    points: &[Point<f32>],
    move_first: bool,
) {
    if points.is_empty() {
        return;
    }
    if move_first {
        builder.move_to(points[0]);
    } else {
        builder.line_to(points[0]);
    }
    if points.len() == 2 {
        builder.line_to(points[1]);
        return;
    }

    // Catmull-Rom to cubic Bezier conversion for silky-smooth audio curves
    let tension = 0.22_f32;
    for i in 0..points.len() - 1 {
        let p0 = if i == 0 { points[0] } else { points[i - 1] };
        let p1 = points[i];
        let p2 = points[i + 1];
        let p3 = if i + 2 < points.len() {
            points[i + 2]
        } else {
            points[points.len() - 1]
        };

        let c1 = Point::new(
            p1.x + (p2.x - p0.x) * tension,
            p1.y + (p2.y - p0.y) * tension,
        );
        let c2 = Point::new(
            p2.x - (p3.x - p1.x) * tension,
            p2.y - (p3.y - p1.y) * tension,
        );
        builder.bezier_curve_to(c1, c2, p2);
    }
}

impl<Message> canvas::Program<Message, cosmic::Theme, cosmic::Renderer> for EqualizerLine<Message> {
    type State = EqualizerLineState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        _bounds: cosmic::iced::Rectangle,
        _cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let mut changed = false;
        if state.springs.len() != self.values.len() {
            state.springs = self
                .values
                .iter()
                .map(|&v| Spring::new(f32::from(v), SpringParams::SNAPPY))
                .collect();
            self.cache.clear();
        } else {
            for (spring, &val) in state.springs.iter_mut().zip(self.values.iter()) {
                let target = f32::from(val);
                if (spring.target - target).abs() > 0.01 {
                    spring.set_target(target);
                    changed = true;
                }
            }
        }

        if let canvas::Event::Window(window::Event::RedrawRequested(now)) = event {
            let dt = state
                .last_tick
                .map_or(1.0 / 60.0, |prev| now.duration_since(prev).as_secs_f32());
            state.last_tick = Some(*now);

            let mut any_moving = false;
            for spring in &mut state.springs {
                if spring.step(dt) {
                    any_moving = true;
                }
            }
            if any_moving {
                self.cache.clear();
                return Some(canvas::Action::request_redraw());
            } else {
                state.last_tick = None;
                self.cache.clear();
            }
        } else if changed || state.springs.iter().any(|s| !s.is_settled()) {
            if state.last_tick.is_none() {
                state.last_tick = Some(Instant::now());
            }
            self.cache.clear();
            return Some(canvas::Action::request_redraw());
        }

        None
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &cosmic::Renderer,
        theme: &cosmic::Theme,
        bounds: cosmic::iced::Rectangle,
        _cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Vec<canvas::Geometry<cosmic::iced::Renderer>> {
        self.sync_theme_cache(theme);
        let animated_values: Vec<f32> = if state.springs.len() == self.values.len() {
            state.springs.iter().map(|s| s.value).collect()
        } else {
            self.values.iter().map(|&v| f32::from(v)).collect()
        };

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

            let points = normalize_float_points(
                &animated_values,
                self.min,
                self.max,
                frame.width(),
                frame.height(),
                padding,
            );
            if points.is_empty() {
                return;
            }

            let stroke_color: Color = match self.variant {
                EqualizerLineVariant::Hero => p.accent_text,
                EqualizerLineVariant::Thumbnail { active: true } => p.accent_text,
                EqualizerLineVariant::Thumbnail { active: false } => p.text_secondary,
            };

            if matches!(self.variant, EqualizerLineVariant::Hero) && points.len() >= 2 {
                let area = canvas::Path::new(|builder| {
                    builder.move_to(Point::new(points[0].x, zero_y));
                    trace_smooth_curve(builder, &points, false);
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

            if points.len() >= 2 {
                let curve = canvas::Path::new(|builder| {
                    trace_smooth_curve(builder, &points, true);
                });
                frame.stroke(
                    &curve,
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
