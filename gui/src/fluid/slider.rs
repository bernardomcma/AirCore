//! Apple-inspired fluid slider with instant press-down thumb expansion,
//! 1:1 drag tracking, and smooth spring glide when clicking the track or
//! updating externally (e.g. EQ preset switches).

use std::{ops::RangeInclusive, time::Instant};

use cosmic::{
    Element, Renderer, Theme,
    iced::{
        Background, Border, Color, Length, Rectangle, Shadow, Size, Vector,
        advanced::{
            Clipboard, Layout, Renderer as _, Shell, Widget, layout, mouse, renderer,
            widget::{Tree, tree},
        },
        border::Radius,
        event::Event,
        window,
    },
};

use crate::{
    spring::{Spring, SpringParams, lerp_color},
    theme,
};

const WIDGET_HEIGHT: f32 = 28.0;
const BASE_TRACK_HEIGHT: f32 = 4.0;
const HOVER_TRACK_HEIGHT: f32 = 5.5;
const BASE_THUMB_SIZE: f32 = 18.0;
const PRESSED_THUMB_SIZE: f32 = 22.0;

pub trait SliderScalar: Copy + PartialOrd + 'static {
    fn to_f64(self) -> f64;
    fn from_f64_stepped(val: f64, step: Self, min: Self, max: Self) -> Self;
    fn default_step() -> Self;
}

impl SliderScalar for i32 {
    fn to_f64(self) -> f64 {
        f64::from(self)
    }

    fn from_f64_stepped(val: f64, step: Self, min: Self, max: Self) -> Self {
        let step_f = f64::from(step.max(1));
        let min_f = f64::from(min);
        let stepped = ((val - min_f) / step_f).round() * step_f + min_f;
        (stepped.round() as i32).clamp(min, max)
    }

    fn default_step() -> Self {
        1
    }
}

impl SliderScalar for i16 {
    fn to_f64(self) -> f64 {
        f64::from(self)
    }

    fn from_f64_stepped(val: f64, step: Self, min: Self, max: Self) -> Self {
        let step_f = f64::from(step.max(1));
        let min_f = f64::from(min);
        let stepped = ((val - min_f) / step_f).round() * step_f + min_f;
        (stepped.round() as i16).clamp(min, max)
    }

    fn default_step() -> Self {
        1
    }
}

pub fn fluid_slider<'a, T: SliderScalar, Message: 'a>(
    range: RangeInclusive<T>,
    value: T,
    on_change: impl Fn(T) -> Message + 'a,
) -> FluidSliderBuilder<'a, T, Message> {
    FluidSliderBuilder {
        range,
        value,
        step: T::default_step(),
        on_change: Box::new(on_change),
        width: Length::Fill,
    }
}

pub struct FluidSliderBuilder<'a, T, Message> {
    range: RangeInclusive<T>,
    value: T,
    step: T,
    on_change: Box<dyn Fn(T) -> Message + 'a>,
    width: Length,
}

impl<'a, T: SliderScalar, Message: 'a> FluidSliderBuilder<'a, T, Message> {
    #[must_use]
    pub fn step(mut self, step: T) -> Self {
        self.step = step;
        self
    }

    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

impl<'a, T: SliderScalar, Message: 'a> From<FluidSliderBuilder<'a, T, Message>>
    for Element<'a, Message>
{
    fn from(builder: FluidSliderBuilder<'a, T, Message>) -> Self {
        Element::new(FluidSlider {
            range: builder.range,
            value: builder.value,
            step: builder.step,
            on_change: builder.on_change,
            width: builder.width,
        })
    }
}

struct FluidSlider<'a, T, Message> {
    range: RangeInclusive<T>,
    value: T,
    step: T,
    on_change: Box<dyn Fn(T) -> Message + 'a>,
    width: Length,
}

struct SliderState {
    initialized: bool,
    last_norm: f32,
    pos_spring: Spring,
    press_spring: Spring,
    hover_spring: Spring,
    last_tick: Option<Instant>,
    dragging: bool,
}

impl Default for SliderState {
    fn default() -> Self {
        Self {
            initialized: false,
            last_norm: 0.0,
            pos_spring: Spring::new(0.0, SpringParams::new(1.0, 0.16)),
            press_spring: Spring::new(0.0, SpringParams::TACTILE),
            hover_spring: Spring::new(0.0, SpringParams::UI_DEFAULT),
            last_tick: None,
            dragging: false,
        }
    }
}

impl SliderState {
    fn sync_norm(&mut self, norm: f32) {
        if !self.initialized {
            self.initialized = true;
            self.last_norm = norm;
            self.pos_spring.snap_to(norm);
        } else if (self.last_norm - norm).abs() > 0.0001 && !self.dragging {
            self.last_norm = norm;
            self.pos_spring.set_target(norm);
        }
    }

    fn is_animating(&self) -> bool {
        !self.pos_spring.is_settled()
            || !self.press_spring.is_settled()
            || !self.hover_spring.is_settled()
    }
}

impl<'a, T: SliderScalar, Message> FluidSlider<'a, T, Message> {
    fn normalize(&self, val: T) -> f32 {
        let min = self.range.start().to_f64();
        let max = self.range.end().to_f64();
        if (max - min).abs() < f64::EPSILON {
            return 0.0;
        }
        ((val.to_f64() - min) / (max - min)).clamp(0.0, 1.0) as f32
    }

    fn value_from_norm(&self, norm: f32) -> T {
        let min = *self.range.start();
        let max = *self.range.end();
        let raw = min.to_f64() + f64::from(norm.clamp(0.0, 1.0)) * (max.to_f64() - min.to_f64());
        T::from_f64_stepped(raw, self.step, min, max)
    }
}

impl<'a, T: SliderScalar, Message> Widget<Message, Theme, Renderer>
    for FluidSlider<'a, T, Message>
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<SliderState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(SliderState::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Fixed(WIDGET_HEIGHT))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.width, Length::Fixed(WIDGET_HEIGHT))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<SliderState>();
        let norm = self.normalize(self.value);
        state.sync_norm(norm);

        let bounds = layout.bounds();
        let pad_x = PRESSED_THUMB_SIZE * 0.5;
        let track_width = (bounds.width - pad_x * 2.0).max(1.0);

        let norm_at_x =
            |x: f32| -> f32 { ((x - (bounds.x + pad_x)) / track_width).clamp(0.0, 1.0) };

        match event {
            Event::Window(window::Event::RedrawRequested(now)) => {
                let dt = state
                    .last_tick
                    .map_or(1.0 / 60.0, |prev| now.duration_since(prev).as_secs_f32());
                state.last_tick = Some(*now);

                let a = if state.dragging {
                    false
                } else {
                    state.pos_spring.step(dt)
                };
                let b = state.press_spring.step(dt);
                let c = state.hover_spring.step(dt);

                if a || b || c {
                    shell.request_redraw();
                } else {
                    state.last_tick = None;
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let target_hover = if bounds.contains(*position) || state.dragging {
                    1.0
                } else {
                    0.0
                };
                if (state.hover_spring.target - target_hover).abs() > 0.01 {
                    state.hover_spring.set_target(target_hover);
                    shell.request_redraw();
                }

                if state.dragging {
                    let raw_norm = norm_at_x(position.x);
                    let new_val = self.value_from_norm(raw_norm);
                    let snapped_norm = self.normalize(new_val);
                    // 1:1 visual tracking while dragging
                    state.pos_spring.snap_to(raw_norm);
                    state.last_norm = snapped_norm;
                    if new_val != self.value {
                        shell.publish((self.on_change)(new_val));
                    }
                    shell.request_redraw();
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_over(bounds) {
                    state.dragging = true;
                    state.press_spring.set_target(1.0);
                    state.hover_spring.set_target(1.0);

                    let raw_norm = norm_at_x(pos.x);
                    let new_val = self.value_from_norm(raw_norm);
                    let snapped_norm = self.normalize(new_val);
                    state.last_norm = snapped_norm;
                    // Smooth glide to initial click point, then 1:1 on move
                    state.pos_spring.set_target(snapped_norm);
                    if new_val != self.value {
                        shell.publish((self.on_change)(new_val));
                    }

                    shell.request_redraw();
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.dragging {
                    state.dragging = false;
                    state.press_spring.set_target(0.0);
                    let snapped = self.normalize(self.value);
                    state.pos_spring.set_target(snapped);
                    let hovered = cursor.is_over(bounds);
                    state
                        .hover_spring
                        .set_target(if hovered { 1.0 } else { 0.0 });
                    shell.request_redraw();
                    shell.capture_event();
                }
            }
            _ => {}
        }

        if state.is_animating() && state.last_tick.is_none() {
            state.last_tick = Some(Instant::now());
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<SliderState>();
        if state.dragging {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<SliderState>();
        let bounds = layout.bounds();
        let p = theme::palette(theme);

        let pos = state.pos_spring.value.clamp(0.0, 1.0);
        let press = state.press_spring.value.clamp(0.0, 1.0);
        let hover = state.hover_spring.value.clamp(0.0, 1.0);

        let pad_x = PRESSED_THUMB_SIZE * 0.5;
        let track_width = (bounds.width - pad_x * 2.0).max(1.0);
        let track_height =
            BASE_TRACK_HEIGHT + (HOVER_TRACK_HEIGHT - BASE_TRACK_HEIGHT) * hover.max(press);
        let track_x = bounds.x + pad_x;
        let track_y = bounds.y + (bounds.height - track_height) * 0.5;

        // 1. Background track
        let track_bg = lerp_color(p.recessed, p.control_border, hover * 0.4);
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: track_x,
                    y: track_y,
                    width: track_width,
                    height: track_height,
                },
                border: Border {
                    radius: Radius::from(track_height * 0.5),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::default(),
                snap: false,
            },
            Background::Color(track_bg),
        );

        // 2. Active filled track
        let fill_width = (pos * track_width).max(0.0);
        if fill_width > 0.5 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: track_x,
                        y: track_y,
                        width: fill_width,
                        height: track_height,
                    },
                    border: Border {
                        radius: Radius::from(track_height * 0.5),
                        width: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    shadow: Shadow::default(),
                    snap: false,
                },
                Background::Color(p.accent),
            );
        }

        let thumb_cx = track_x + pos * track_width;
        let thumb_cy = bounds.y + bounds.height * 0.5;

        // 3. Subtle focus/press halo
        let halo_alpha = (hover * 0.10 + press * 0.16).clamp(0.0, 0.25);
        if halo_alpha > 0.005 {
            let halo_size = PRESSED_THUMB_SIZE + 8.0 * press;
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: thumb_cx - halo_size * 0.5,
                        y: thumb_cy - halo_size * 0.5,
                        width: halo_size,
                        height: halo_size,
                    },
                    border: Border {
                        radius: Radius::from(halo_size * 0.5),
                        width: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    shadow: Shadow::default(),
                    snap: false,
                },
                Background::Color(Color {
                    a: halo_alpha,
                    ..p.accent
                }),
            );
        }

        // 4. Thumb knob
        let thumb_size = BASE_THUMB_SIZE + (PRESSED_THUMB_SIZE - BASE_THUMB_SIZE) * press;
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: thumb_cx - thumb_size * 0.5,
                    y: thumb_cy - thumb_size * 0.5,
                    width: thumb_size,
                    height: thumb_size,
                },
                border: Border {
                    radius: Radius::from(thumb_size * 0.5),
                    width: 0.75,
                    color: if p.is_dark {
                        Color::from_rgba8(255, 255, 255, 0.18)
                    } else {
                        Color::from_rgba8(0, 0, 0, 0.10)
                    },
                },
                shadow: Shadow {
                    color: Color::from_rgba8(0, 0, 0, if p.is_dark { 0.35 } else { 0.18 }),
                    offset: Vector::new(0.0, 1.5),
                    blur_radius: 5.0 + 3.0 * press,
                },
                snap: false,
            },
            Background::Color(Color::WHITE),
        );
    }
}
