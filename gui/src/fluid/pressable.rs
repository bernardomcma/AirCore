//! Tactile pressable wrapper that gives any button or card instant Apple-style
//! pointer-down scale feedback (`1.0 -> 0.968`) and spring release bounce.

use std::time::Instant;

use cosmic::{
    Element, Renderer, Theme,
    iced::{
        Length, Rectangle, Size, Transformation, Vector,
        advanced::{
            Clipboard, Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer,
            widget::{self, Tree, tree},
        },
        event::Event,
        window,
    },
};

use crate::spring::{Spring, SpringParams};

pub fn fluid_pressable<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    Element::new(FluidPressable {
        content: content.into(),
        press_scale: 0.968,
    })
}

struct FluidPressable<'a, Message> {
    content: Element<'a, Message>,
    press_scale: f32,
}

struct PressableState {
    scale_spring: Spring,
    last_tick: Option<Instant>,
    pressed: bool,
}

impl Default for PressableState {
    fn default() -> Self {
        Self {
            scale_spring: Spring::new(1.0, SpringParams::TACTILE),
            last_tick: None,
            pressed: false,
        }
    }
}

impl<'a, Message> Widget<Message, Theme, Renderer> for FluidPressable<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<PressableState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(PressableState::default())
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
        let state = tree.state.downcast_mut::<PressableState>();
        let bounds = layout.bounds();

        match event {
            Event::Window(window::Event::RedrawRequested(now)) => {
                let dt = state
                    .last_tick
                    .map_or(1.0 / 60.0, |prev| now.duration_since(prev).as_secs_f32());
                state.last_tick = Some(*now);
                if state.scale_spring.step(dt) {
                    shell.request_redraw();
                } else {
                    state.last_tick = None;
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if cursor.is_over(bounds) {
                    state.pressed = true;
                    state.scale_spring.set_target(self.press_scale);
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.pressed {
                    state.pressed = false;
                    state.scale_spring.set_target(1.0);
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.pressed => {
                let target = if bounds.contains(*position) {
                    self.press_scale
                } else {
                    1.0
                };
                if (state.scale_spring.target - target).abs() > 0.001 {
                    state.scale_spring.set_target(target);
                    shell.request_redraw();
                }
            }
            _ => {}
        }

        // Forward event to child so inner button/clickable still works normally
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

        if !state.scale_spring.is_settled() && state.last_tick.is_none() {
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
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
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
        let state = tree.state.downcast_ref::<PressableState>();
        let scale = state.scale_spring.value;
        let bounds = layout.bounds();

        if (scale - 1.0).abs() < 0.0005 {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
        } else {
            let cx = bounds.x + bounds.width * 0.5;
            let cy = bounds.y + bounds.height * 0.5;
            let transform = Transformation::translate(cx, cy)
                * Transformation::scale(scale)
                * Transformation::translate(-cx, -cy);

            renderer.with_transformation(transform, |renderer| {
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
        }
    }
}
