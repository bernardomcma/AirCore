//! iOS 26 / iOS 27 fluid pill-shaped segmented control.
//!
//! Features:
//! - Floating active pill that glides between segments with interruptible spring physics.
//! - 1:1 pointer drag/scrub across segments with edge rubber-banding and momentum projection.
//! - Instant press-down squish on pointer-down for immediate tactile feedback.

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

use crate::spring::{Spring, SpringParams, project_momentum, rubberband};

const TRACK_PADDING: f32 = 4.0;
const SEGMENT_GAP: f32 = 4.0;

pub fn fluid_segmented_control<'a, Message: Clone + 'a>(
    segments: Vec<Element<'a, Message>>,
    selected_index: Option<usize>,
    on_select: impl Fn(usize) -> Message + 'a,
) -> Element<'a, Message> {
    Element::new(FluidSegmentedControl {
        segments,
        selected_index,
        on_select: Box::new(on_select),
    })
}

struct FluidSegmentedControl<'a, Message> {
    segments: Vec<Element<'a, Message>>,
    selected_index: Option<usize>,
    on_select: Box<dyn Fn(usize) -> Message + 'a>,
}

struct SegmentedState {
    initialized: bool,
    last_selected: Option<usize>,
    pos_spring: Spring,
    press_spring: Spring,
    pressed_idx: Option<usize>,
    last_tick: Option<Instant>,
    dragging: bool,
    drag_start_x: f32,
    drag_start_pos: f32,
    drag_moved: bool,
    last_cursor_x: f32,
    last_move_time: Option<Instant>,
    velocity_x: f32,
}

impl Default for SegmentedState {
    fn default() -> Self {
        Self {
            initialized: false,
            last_selected: None,
            pos_spring: Spring::new(0.0, SpringParams::SNAPPY),
            press_spring: Spring::new(0.0, SpringParams::TACTILE),
            pressed_idx: None,
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

impl SegmentedState {
    fn sync_selected(&mut self, selected: Option<usize>) {
        if !self.initialized {
            self.initialized = true;
            self.last_selected = selected;
            if let Some(idx) = selected {
                self.pos_spring.snap_to(idx as f32);
            }
        } else if self.last_selected != selected && !self.dragging {
            self.last_selected = selected;
            if let Some(idx) = selected {
                self.pos_spring.set_target(idx as f32);
            }
        }
    }

    fn is_animating(&self) -> bool {
        !self.pos_spring.is_settled() || !self.press_spring.is_settled()
    }
}

impl<'a, Message: Clone> Widget<Message, Theme, Renderer> for FluidSegmentedControl<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<SegmentedState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(SegmentedState::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.segments.iter().map(Tree::new).collect()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.segments);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let count = self.segments.len().max(1);
        let max_size = limits.max();
        let total_width = if max_size.width.is_finite() {
            max_size.width
        } else {
            320.0
        };

        let inner_width = (total_width - TRACK_PADDING * 2.0).max(0.0);
        let total_gaps = SEGMENT_GAP * (count.saturating_sub(1) as f32);
        let seg_width = ((inner_width - total_gaps) / count as f32).max(1.0);

        let child_limits = layout::Limits::new(
            Size::new(seg_width, 0.0),
            Size::new(seg_width, max_size.height),
        );

        let mut nodes = Vec::with_capacity(self.segments.len());
        let mut max_child_height: f32 = 34.0;

        for (child, child_tree) in self.segments.iter_mut().zip(tree.children.iter_mut()) {
            let node = child
                .as_widget_mut()
                .layout(child_tree, renderer, &child_limits);
            max_child_height = max_child_height.max(node.size().height);
            nodes.push(node);
        }

        let mut positioned = Vec::with_capacity(nodes.len());
        for (i, node) in nodes.into_iter().enumerate() {
            let x = TRACK_PADDING + i as f32 * (seg_width + SEGMENT_GAP);
            let y = TRACK_PADDING + (max_child_height - node.size().height) * 0.5;
            positioned.push(node.move_to(cosmic::iced::Point::new(x, y)));
        }

        layout::Node::with_children(
            Size::new(total_width, max_child_height + TRACK_PADDING * 2.0),
            positioned,
        )
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
        let state = tree.state.downcast_mut::<SegmentedState>();
        state.sync_selected(self.selected_index);

        let count = self.segments.len();
        if count == 0 {
            return;
        }
        let max_idx = (count - 1) as f32;
        let bounds = layout.bounds();
        let inner_width = (bounds.width - TRACK_PADDING * 2.0).max(1.0);
        let total_gaps = SEGMENT_GAP * (count.saturating_sub(1) as f32);
        let seg_width = ((inner_width - total_gaps) / count as f32).max(1.0);
        let step_x = seg_width + SEGMENT_GAP;

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

                if a || b {
                    shell.request_redraw();
                } else {
                    state.last_tick = None;
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_over(bounds) {
                    let rel_x = pos.x - (bounds.x + TRACK_PADDING);
                    let idx = ((rel_x / step_x).floor() as usize).min(count - 1);
                    state.dragging = true;
                    state.drag_moved = false;
                    state.drag_start_x = pos.x;
                    state.drag_start_pos = state.pos_spring.value;
                    state.last_cursor_x = pos.x;
                    state.last_move_time = Some(Instant::now());
                    state.velocity_x = 0.0;
                    state.pressed_idx = Some(idx);
                    state.press_spring.set_target(1.0);
                    shell.request_redraw();
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if state.dragging {
                    let dx = position.x - state.drag_start_x;
                    if dx.abs() > 4.0 {
                        state.drag_moved = true;
                    }
                    if state.drag_moved {
                        let raw_pos = state.drag_start_pos + dx / step_x;
                        let pos_val = if raw_pos < 0.0 {
                            rubberband(raw_pos, 1.0, 0.35)
                        } else if raw_pos > max_idx {
                            max_idx + rubberband(raw_pos - max_idx, 1.0, 0.35)
                        } else {
                            raw_pos
                        };
                        state.pos_spring.snap_to(pos_val);

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
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.dragging {
                    state.dragging = false;
                    state.press_spring.set_target(0.0);
                    state.pressed_idx = None;

                    let target_idx = if state.drag_moved {
                        let norm_vel = state.velocity_x / step_x;
                        let projected = state.pos_spring.value + project_momentum(norm_vel, 0.99);
                        let idx = (projected.round() as isize).clamp(0, max_idx as isize) as usize;
                        state.pos_spring.set_velocity(norm_vel);
                        idx
                    } else if let Some(pos) = cursor.position_over(Rectangle {
                        x: bounds.x - 6.0,
                        y: bounds.y - 6.0,
                        width: bounds.width + 12.0,
                        height: bounds.height + 12.0,
                    }) {
                        let rel_x = pos.x - (bounds.x + TRACK_PADDING);
                        ((rel_x / step_x).floor() as isize).clamp(0, max_idx as isize) as usize
                    } else {
                        self.selected_index.unwrap_or(0)
                    };

                    state.last_selected = Some(target_idx);
                    state.pos_spring.set_target(target_idx as f32);
                    if Some(target_idx) != self.selected_index {
                        shell.publish((self.on_select)(target_idx));
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
        let state = tree.state.downcast_ref::<SegmentedState>();
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
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<SegmentedState>();
        let bounds = layout.bounds();
        let is_dark = theme.cosmic().is_dark;
        let count = self.segments.len();

        let track_bg = if is_dark {
            Color::from_rgb8(44, 44, 48)
        } else {
            Color::from_rgb8(229, 229, 234)
        };
        let track_border = if is_dark {
            Color::from_rgb8(62, 62, 66)
        } else {
            Color::from_rgb8(216, 216, 222)
        };

        // 1. Stadium track
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    radius: Radius::from(999.0),
                    width: 1.0,
                    color: track_border,
                },
                shadow: Shadow::default(),
                snap: true,
            },
            Background::Color(track_bg),
        );

        // 2. Floating sliding pill
        if count > 0 && self.selected_index.is_some() {
            let inner_width = (bounds.width - TRACK_PADDING * 2.0).max(1.0);
            let total_gaps = SEGMENT_GAP * (count.saturating_sub(1) as f32);
            let seg_width = ((inner_width - total_gaps) / count as f32).max(1.0);
            let step_x = seg_width + SEGMENT_GAP;

            let press = state.press_spring.value.clamp(0.0, 1.0);
            let squish_x = 2.0 * press;
            let squish_y = 1.5 * press;

            let pill_x = bounds.x + TRACK_PADDING + state.pos_spring.value * step_x + squish_x;
            let pill_y = bounds.y + TRACK_PADDING + squish_y;
            let pill_w = (seg_width - squish_x * 2.0).max(8.0);
            let pill_h = (bounds.height - TRACK_PADDING * 2.0 - squish_y * 2.0).max(8.0);

            let pill_bg = if is_dark {
                Color::from_rgb8(76, 76, 82)
            } else {
                Color::WHITE
            };
            let pill_border = if is_dark {
                Color::from_rgb8(98, 98, 104)
            } else {
                Color::from_rgb8(220, 220, 226)
            };

            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: pill_x,
                        y: pill_y,
                        width: pill_w,
                        height: pill_h,
                    },
                    border: Border {
                        radius: Radius::from(999.0),
                        width: 0.75,
                        color: pill_border,
                    },
                    shadow: Shadow {
                        color: Color::from_rgba8(0, 0, 0, if is_dark { 0.30 } else { 0.10 }),
                        offset: Vector::new(0.0, 2.0),
                        blur_radius: 8.0,
                    },
                    snap: false,
                },
                Background::Color(pill_bg),
            );
        }

        // 3. Draw segment children (icons or captions) on top
        for ((child, child_tree), child_layout) in self
            .segments
            .iter()
            .zip(tree.children.iter())
            .zip(layout.children())
        {
            child.as_widget().draw(
                child_tree,
                renderer,
                theme,
                style,
                child_layout,
                cursor,
                viewport,
            );
        }
    }
}
