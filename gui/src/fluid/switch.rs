//! iOS-style fluid toggle switch with press-down thumb stretch, 1:1 drag,
//! velocity handoff, rubber-banding, and interruptible spring motion.

use std::time::Instant;

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

use crate::spring::{Spring, SpringParams, lerp_color, project_momentum, rubberband};

const TRACK_WIDTH: f32 = 48.0;
const TRACK_HEIGHT: f32 = 28.0;
const THUMB_SIZE: f32 = 24.0;
const THUMB_STRETCH: f32 = 5.5;
const TRACK_PADDING: f32 = 2.0;

pub fn fluid_switch<'a, Message: 'a>(
    value: bool,
    on_toggle: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    Element::new(FluidSwitch {
        value,
        on_toggle: Box::new(on_toggle),
    })
}

struct FluidSwitch<'a, Message> {
    value: bool,
    on_toggle: Box<dyn Fn(bool) -> Message + 'a>,
}

struct SwitchState {
    initialized: bool,
    last_value: bool,
    pos_spring: Spring,
    press_spring: Spring,
    hover_spring: Spring,
    last_tick: Option<Instant>,
    dragging: bool,
    drag_start_x: f32,
    drag_start_pos: f32,
    drag_moved: bool,
    last_cursor_x: f32,
    last_move_time: Option<Instant>,
    velocity_x: f32,
}

impl Default for SwitchState {
    fn default() -> Self {
        Self {
            initialized: false,
            last_value: false,
            pos_spring: Spring::new(0.0, SpringParams::SNAPPY),
            press_spring: Spring::new(0.0, SpringParams::TACTILE),
            hover_spring: Spring::new(0.0, SpringParams::UI_DEFAULT),
            last_tick: None,
            dragging: false,
            drag_start_x: 0.0,
            drag_start_pos: 0.0,
            drag_moved: false,
            last_cursor_x: 0.0,
            last_move_time: None,
            velocity_x: 0.0,
        }
    }
}

impl SwitchState {
    fn sync_value(&mut self, value: bool) {
        let target = if value { 1.0 } else { 0.0 };
        if !self.initialized {
            self.initialized = true;
            self.last_value = value;
            self.pos_spring.snap_to(target);
        } else if self.last_value != value && !self.dragging {
            self.last_value = value;
            self.pos_spring.set_target(target);
        }
    }

    fn is_animating(&self) -> bool {
        !self.pos_spring.is_settled()
            || !self.press_spring.is_settled()
            || !self.hover_spring.is_settled()
    }
}

impl<'a, Message> Widget<Message, Theme, Renderer> for FluidSwitch<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<SwitchState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(SwitchState::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(TRACK_WIDTH), Length::Fixed(TRACK_HEIGHT))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, TRACK_WIDTH, TRACK_HEIGHT)
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
        let state = tree.state.downcast_mut::<SwitchState>();
        state.sync_value(self.value);

        let bounds = layout.bounds();
        let travel = TRACK_WIDTH - TRACK_PADDING * 2.0 - THUMB_SIZE;

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
                let hovered = bounds.contains(*position);
                let target_hover = if hovered { 1.0 } else { 0.0 };
                if (state.hover_spring.target - target_hover).abs() > 0.01 {
                    state.hover_spring.set_target(target_hover);
                    shell.request_redraw();
                }

                if state.dragging {
                    let dx = position.x - state.drag_start_x;
                    if dx.abs() > 3.0 {
                        state.drag_moved = true;
                    }
                    if state.drag_moved {
                        let raw_pos = state.drag_start_pos + dx / travel;
                        let clamped_or_rubber = if raw_pos < 0.0 {
                            rubberband(raw_pos, 1.0, 0.35)
                        } else if raw_pos > 1.0 {
                            1.0 + rubberband(raw_pos - 1.0, 1.0, 0.35)
                        } else {
                            raw_pos
                        };
                        state.pos_spring.snap_to(clamped_or_rubber);

                        let now = Instant::now();
                        if let Some(prev_t) = state.last_move_time {
                            let dt = now.duration_since(prev_t).as_secs_f32();
                            if dt > 0.001 {
                                let inst_v = (position.x - state.last_cursor_x) / dt;
                                state.velocity_x = state.velocity_x * 0.4 + inst_v * 0.6;
                            }
                        }
                        state.last_cursor_x = position.x;
                        state.last_move_time = Some(now);
                        shell.request_redraw();
                    }
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_over(bounds) {
                    state.dragging = true;
                    state.drag_moved = false;
                    state.drag_start_x = pos.x;
                    state.drag_start_pos = state.pos_spring.value;
                    state.last_cursor_x = pos.x;
                    state.last_move_time = Some(Instant::now());
                    state.velocity_x = 0.0;
                    state.press_spring.set_target(1.0);
                    shell.request_redraw();
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.dragging {
                    state.dragging = false;
                    state.press_spring.set_target(0.0);

                    if state.drag_moved {
                        let norm_vel = state.velocity_x / travel;
                        let projected = state.pos_spring.value + project_momentum(norm_vel, 0.99);
                        let next_value = projected >= 0.5;
                        state
                            .pos_spring
                            .set_target(if next_value { 1.0 } else { 0.0 });
                        state.pos_spring.set_velocity(norm_vel);
                        if next_value != self.value {
                            state.last_value = next_value;
                            shell.publish((self.on_toggle)(next_value));
                        }
                    } else {
                        let hit = Rectangle {
                            x: bounds.x - 8.0,
                            y: bounds.y - 8.0,
                            width: bounds.width + 16.0,
                            height: bounds.height + 16.0,
                        };
                        if cursor.position_over(hit).is_some() {
                            let next_value = !self.value;
                            state.last_value = next_value;
                            state
                                .pos_spring
                                .set_target(if next_value { 1.0 } else { 0.0 });
                            shell.publish((self.on_toggle)(next_value));
                        }
                    }

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
        let state = tree.state.downcast_ref::<SwitchState>();
        if state.dragging || cursor.is_over(layout.bounds()) {
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
        let state = tree.state.downcast_ref::<SwitchState>();
        let bounds = layout.bounds();
        let is_dark = theme.cosmic().is_dark;

        let pos = state.pos_spring.value;
        let clamped_pos = pos.clamp(0.0, 1.0);
        let press = state.press_spring.value.clamp(0.0, 1.0);
        let hover = state.hover_spring.value.clamp(0.0, 1.0);

        let off_track = if is_dark {
            Color::from_rgb8(58, 58, 62)
        } else {
            Color::from_rgb8(216, 216, 222)
        };
        let off_track_hover = if is_dark {
            Color::from_rgb8(70, 70, 76)
        } else {
            Color::from_rgb8(204, 204, 212)
        };
        let on_track = if is_dark {
            Color::from_rgb8(48, 209, 88)
        } else {
            Color::from_rgb8(52, 199, 89)
        };

        let base_off = lerp_color(off_track, off_track_hover, hover);
        let track_color = lerp_color(base_off, on_track, clamped_pos);

        // Track pill
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    radius: Radius::from(TRACK_HEIGHT * 0.5),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::default(),
                snap: true,
            },
            Background::Color(track_color),
        );

        // Thumb stretches horizontally when pressed (authentic iOS UISwitch feel)
        let thumb_width = THUMB_SIZE + THUMB_STRETCH * press;
        let travel = TRACK_WIDTH - TRACK_PADDING * 2.0 - thumb_width;
        let thumb_x = bounds.x + TRACK_PADDING + pos * travel;
        let thumb_y = bounds.y + TRACK_PADDING;

        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: thumb_x,
                    y: thumb_y,
                    width: thumb_width,
                    height: THUMB_SIZE,
                },
                border: Border {
                    radius: Radius::from(THUMB_SIZE * 0.5),
                    width: 0.5,
                    color: Color::from_rgba8(0, 0, 0, if is_dark { 0.18 } else { 0.06 }),
                },
                shadow: Shadow {
                    color: Color::from_rgba8(0, 0, 0, if is_dark { 0.35 } else { 0.16 }),
                    offset: Vector::new(0.0, 1.5),
                    blur_radius: 5.0,
                },
                snap: false,
            },
            Background::Color(Color::WHITE),
        );
    }
}
