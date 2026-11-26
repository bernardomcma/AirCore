//! Smooth Apple-style spring slide + dissolve transition when a view key changes
//! (e.g. switching device settings categories or navigating between screens).

use std::time::Instant;

use cosmic::{
    Element, Renderer, Theme,
    iced::{
        Background, Border, Color, Length, Rectangle, Shadow, Size, Transformation, Vector,
        advanced::{
            Clipboard, Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer,
            widget::{self, Tree, tree},
        },
        border::Radius,
        event::Event,
        window,
    },
};

use crate::{
    spring::{Spring, SpringParams},
    theme,
};

pub fn fluid_transition<'a, Message: 'a>(
    key: u64,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    Element::new(FluidTransition {
        key,
        content: content.into(),
        slide_distance: 10.0,
    })
}

struct FluidTransition<'a, Message> {
    key: u64,
    content: Element<'a, Message>,
    slide_distance: f32,
}

struct TransitionState {
    initialized: bool,
    last_key: u64,
    progress: Spring,
    last_tick: Option<Instant>,
}

impl Default for TransitionState {
    fn default() -> Self {
        Self {
            initialized: false,
            last_key: 0,
            progress: Spring::new(1.0, SpringParams::new(1.0, 0.22)),
            last_tick: None,
        }
    }
}

impl<'a, Message> Widget<Message, Theme, Renderer> for FluidTransition<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<TransitionState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(TransitionState::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<TransitionState>();
        if !state.initialized {
            state.initialized = true;
            state.last_key = self.key;
            state.progress.snap_to(1.0);
        } else if state.last_key != self.key {
            state.last_key = self.key;
            state.progress.snap_to(0.0);
            state.progress.set_target(1.0);
            state.last_tick = Some(Instant::now());
            shell.request_redraw();
        }

        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            let dt = state
                .last_tick
                .map_or(1.0 / 60.0, |prev| now.duration_since(prev).as_secs_f32());
            state.last_tick = Some(*now);
            if state.progress.step(dt) {
                shell.request_redraw();
            } else {
                state.last_tick = None;
            }
        }

        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        if !state.progress.is_settled() && state.last_tick.is_none() {
            state.last_tick = Some(Instant::now());
            shell.request_redraw();
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let state = tree.state.downcast_ref::<TransitionState>();
        let t = state.progress.value.clamp(0.0, 1.0);
        let offset_y = if t >= 0.999 {
            0.0
        } else {
            self.slide_distance * (1.0 - t)
        };
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation + Vector::new(0.0, offset_y),
        )
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
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
        let state = tree.state.downcast_ref::<TransitionState>();
        let t = state.progress.value.clamp(0.0, 1.0);

        if t >= 0.999 {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
            return;
        }

        let offset_y = self.slide_distance * (1.0 - t);
        let bounds = layout.bounds();

        renderer.with_layer(bounds, |renderer| {
            renderer.with_transformation(Transformation::translate(0.0, offset_y), |renderer| {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    layout,
                    cursor,
                    viewport,
                );
            });

            let p = theme::palette(theme);
            let veil_alpha = (1.0 - t) * 0.72;
            if veil_alpha > 0.005 {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        border: Border {
                            radius: Radius::from(0.0),
                            width: 0.0,
                            color: Color::TRANSPARENT,
                        },
                        shadow: Shadow::default(),
                        snap: false,
                    },
                    Background::Color(Color {
                        a: veil_alpha,
                        ..p.canvas
                    }),
                );
            }
        });
    }
}
