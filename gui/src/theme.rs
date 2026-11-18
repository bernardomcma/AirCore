//! Centralized Apple-inspired theme tokens, surface styles, and widget themes.
//!
//! Adapted natively to `libcosmic` 1.0 (`iced` 0.14) from `Leonxlnx/taste-skill`
//! (`.agents/skills/design-taste-frontend/SKILL.md` & app-screen discipline)
//! and Apple Human Interface Guidelines:
//! - Flat tone-stepped surfaces (`canvas` -> `card` / `recessed`) separated by 1px
//!   hairline borders with zero resting card shadows or gradients.
//! - Disciplined `< 5%` semantic accent budget reserved for primary actions and
//!   key data visualization (such as the active EQ curve and selected presets).
//! - Consistent 4/8/12/16/24/32 spacing scale and crisp radii (`8 / 12 / 16 / pill`).
//! - Monospace/tabular numerals for dB, Hz, MAC addresses, and numeric readouts.
//! - Guaranteed WCAG AA contrast across both Light and Dark themes.

use std::borrow::Cow;
use std::rc::Rc;
use std::sync::Arc;

use cosmic::{
    Theme,
    cosmic_theme::{
        ThemeBuilder,
        palette::{Srgb, Srgba},
    },
    iced::{
        Background, Border, Color, Shadow, Vector,
        border::Radius,
        widget::{container, rule, slider, text},
    },
    widget::{self, button, text_input},
};

pub const SPACE_XS: u16 = 4;
pub const SPACE_SM: u16 = 8;
pub const SPACE_MD: u16 = 12;
pub const SPACE_LG: u16 = 16;
pub const SPACE_XL: u16 = 24;

pub const RADIUS_SM: f32 = 8.0;
pub const RADIUS_MD: f32 = 12.0;
pub const RADIUS_PILL: f32 = 999.0;

pub const MAX_CONTENT_WIDTH: f32 = 880.0;
pub const NARROW_BREAKPOINT: f32 = 450.0;
pub const SIDEBAR_BREAKPOINT: f32 = 680.0;
pub const DRAWER_SINGLE_PANE_BREAKPOINT: f32 = 720.0;
pub const EQ_TWO_COLUMN_BREAKPOINT: f32 = 960.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticTone {
    Neutral,
    Accent,
    Warning,
}

#[derive(Debug, Clone, Copy)]
pub struct AppPalette {
    pub is_dark: bool,
    pub canvas: Color,
    pub card: Color,
    pub recessed: Color,
    pub sidebar: Color,
    pub border: Color,
    pub divider: Color,
    pub control_border: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_tertiary: Color,
    pub accent: Color,
    pub accent_text: Color,
    pub on_accent: Color,
    pub accent_tint: Color,
    pub warning_tint: Color,
    pub warning_border: Color,
    pub warning_text: Color,
    pub destructive: Color,
    pub destructive_tint: Color,
    pub destructive_text: Color,
    pub hover_overlay: Color,
    pub pressed_overlay: Color,
    pub selected_neutral: Color,
}

impl AppPalette {
    #[must_use]
    pub fn light() -> Self {
        Self {
            is_dark: false,
            canvas: Color::from_rgb8(245, 245, 247),
            card: Color::from_rgb8(255, 255, 255),
            recessed: Color::from_rgb8(242, 242, 247),
            sidebar: Color::from_rgb8(255, 255, 255),
            border: Color::from_rgb8(229, 229, 234),
            divider: Color::from_rgb8(235, 235, 240),
            control_border: Color::from_rgb8(209, 209, 214),
            text_primary: Color::from_rgb8(29, 29, 31),
            text_secondary: Color::from_rgb8(76, 76, 82),
            text_tertiary: Color::from_rgb8(88, 88, 94),
            accent: Color::from_rgb8(0, 96, 198),
            accent_text: Color::from_rgb8(0, 90, 186),
            on_accent: Color::WHITE,
            accent_tint: Color::from_rgb8(235, 244, 255),
            warning_tint: Color::from_rgb8(255, 251, 235),
            warning_border: Color::from_rgb8(252, 211, 77),
            warning_text: Color::from_rgb8(133, 77, 14),
            destructive: Color::from_rgb8(178, 22, 32),
            destructive_tint: Color::from_rgb8(245, 245, 247),
            destructive_text: Color::from_rgb8(152, 18, 26),
            hover_overlay: Color::from_rgba8(0, 0, 0, 0.035),
            pressed_overlay: Color::from_rgba8(0, 0, 0, 0.07),
            selected_neutral: Color::from_rgb8(232, 242, 255),
        }
    }

    #[must_use]
    pub fn dark() -> Self {
        Self {
            is_dark: true,
            canvas: Color::from_rgb8(28, 28, 30),
            card: Color::from_rgb8(44, 44, 46),
            recessed: Color::from_rgb8(36, 36, 38),
            sidebar: Color::from_rgb8(34, 34, 36),
            border: Color::from_rgb8(58, 58, 60),
            divider: Color::from_rgb8(52, 52, 56),
            control_border: Color::from_rgb8(72, 72, 74),
            text_primary: Color::from_rgb8(245, 245, 247),
            text_secondary: Color::from_rgb8(198, 198, 204),
            text_tertiary: Color::from_rgb8(178, 178, 184),
            accent: Color::from_rgb8(0, 92, 196),
            accent_text: Color::from_rgb8(90, 169, 255),
            on_accent: Color::WHITE,
            accent_tint: Color::from_rgb8(22, 46, 74),
            warning_tint: Color::from_rgb8(46, 35, 18),
            warning_border: Color::from_rgb8(102, 75, 24),
            warning_text: Color::from_rgb8(251, 191, 36),
            destructive: Color::from_rgb8(186, 26, 20),
            destructive_tint: Color::from_rgb8(46, 20, 20),
            destructive_text: Color::from_rgb8(255, 138, 132),
            hover_overlay: Color::from_rgba8(255, 255, 255, 0.06),
            pressed_overlay: Color::from_rgba8(255, 255, 255, 0.10),
            selected_neutral: Color::from_rgb8(58, 58, 60),
        }
    }
}

pub fn apple_light_theme() -> Theme {
    let built = ThemeBuilder::light()
        .bg_color(Srgba::new(1.0, 1.0, 1.0, 1.0))
        .primary_container_bg(Srgba::new(1.0, 1.0, 1.0, 1.0))
        .text_tint(Srgb::new(29.0 / 255.0, 29.0 / 255.0, 31.0 / 255.0))
        .neutral_tint(Srgb::new(110.0 / 255.0, 110.0 / 255.0, 115.0 / 255.0))
        .accent(Srgb::new(0.0, 113.0 / 255.0, 227.0 / 255.0))
        .success(Srgb::new(52.0 / 255.0, 199.0 / 255.0, 89.0 / 255.0))
        .warning(Srgb::new(1.0, 149.0 / 255.0, 0.0))
        .destructive(Srgb::new(225.0 / 255.0, 45.0 / 255.0, 36.0 / 255.0))
        .build();
    Theme::custom(Arc::new(built))
}

pub fn apple_dark_theme() -> Theme {
    let built = ThemeBuilder::dark()
        .bg_color(Srgba::new(28.0 / 255.0, 28.0 / 255.0, 30.0 / 255.0, 1.0))
        .primary_container_bg(Srgba::new(44.0 / 255.0, 44.0 / 255.0, 46.0 / 255.0, 1.0))
        .text_tint(Srgb::new(245.0 / 255.0, 245.0 / 255.0, 247.0 / 255.0))
        .neutral_tint(Srgb::new(142.0 / 255.0, 142.0 / 255.0, 147.0 / 255.0))
        .accent(Srgb::new(10.0 / 255.0, 132.0 / 255.0, 1.0))
        .success(Srgb::new(48.0 / 255.0, 209.0 / 255.0, 88.0 / 255.0))
        .warning(Srgb::new(1.0, 159.0 / 255.0, 10.0 / 255.0))
        .destructive(Srgb::new(1.0, 69.0 / 255.0, 58.0 / 255.0))
        .build();
    Theme::custom(Arc::new(built))
}

#[must_use]
pub fn palette(theme: &Theme) -> AppPalette {
    if theme.cosmic().is_dark {
        AppPalette::dark()
    } else {
        AppPalette::light()
    }
}

#[must_use]
pub fn canvas_container(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        icon_color: Some(p.text_primary),
        text_color: Some(p.text_primary),
        background: Some(Background::Color(p.canvas)),
        border: Border::default(),
        shadow: Shadow::default(),
        snap: true,
    }
}

#[must_use]
pub fn card_container(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        icon_color: Some(p.text_primary),
        text_color: Some(p.text_primary),
        background: Some(Background::Color(p.card)),
        border: Border {
            color: p.border,
            width: 1.0,
            radius: Radius::from(RADIUS_MD),
        },
        shadow: if p.is_dark {
            Shadow::default()
        } else {
            Shadow {
                color: Color::from_rgba8(15, 23, 42, 0.03),
                offset: Vector::new(0.0, 2.0),
                blur_radius: 8.0,
            }
        },
        snap: true,
    }
}

#[must_use]
pub fn sidebar_container(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        icon_color: Some(p.text_primary),
        text_color: Some(p.text_primary),
        background: Some(Background::Color(p.sidebar)),
        border: Border {
            color: p.border,
            width: 1.0,
            radius: Radius::from(RADIUS_MD),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn pill_badge_container(tone: SemanticTone) -> impl Fn(&Theme) -> container::Style {
    move |theme: &Theme| {
        let p = palette(theme);
        let (bg, fg, border_color) = match tone {
            SemanticTone::Neutral => (p.recessed, p.text_secondary, p.border),
            SemanticTone::Accent => (p.accent_tint, p.accent_text, p.accent_tint),
            SemanticTone::Warning => (p.warning_tint, p.warning_text, p.warning_border),
        };
        container::Style {
            icon_color: Some(fg),
            text_color: Some(fg),
            background: Some(Background::Color(bg)),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: Radius::from(RADIUS_PILL),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn alert_banner_container(tone: SemanticTone) -> impl Fn(&Theme) -> container::Style {
    move |theme: &Theme| {
        let p = palette(theme);
        let (bg, fg, border_color) = match tone {
            SemanticTone::Neutral => (p.card, p.text_primary, p.border),
            SemanticTone::Accent => (p.accent_tint, p.accent_text, p.accent_text),
            SemanticTone::Warning => (p.warning_tint, p.warning_text, p.warning_border),
        };
        container::Style {
            icon_color: Some(fg),
            text_color: Some(fg),
            background: Some(Background::Color(bg)),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: Radius::from(RADIUS_SM),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

#[must_use]
pub fn hairline_rule(theme: &Theme) -> rule::Style {
    let p = palette(theme);
    rule::Style {
        color: p.divider,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

fn base_button_style(radius: f32) -> button::Style {
    button::Style {
        shadow_offset: Vector::new(0.0, 0.0),
        background: None,
        overlay: None,
        border_radius: Radius::from(radius),
        border_width: 0.0,
        border_color: Color::TRANSPARENT,
        outline_width: 0.0,
        outline_color: Color::TRANSPARENT,
        icon_color: None,
        text_color: None,
    }
}

#[must_use]
pub fn primary_button() -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(|focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.background = Some(Background::Color(p.accent));
            style.text_color = Some(p.on_accent);
            style.icon_color = Some(p.on_accent);
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.accent_text;
            }
            style
        }),
        disabled: Box::new(|theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.background = Some(Background::Color(p.recessed));
            style.text_color = Some(p.text_tertiary);
            style.icon_color = Some(p.text_tertiary);
            style.border_width = 1.0;
            style.border_color = p.border;
            style
        }),
        hovered: Box::new(|focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.background = Some(Background::Color(p.accent));
            style.overlay = Some(Background::Color(Color::from_rgba8(255, 255, 255, 0.10)));
            style.text_color = Some(p.on_accent);
            style.icon_color = Some(p.on_accent);
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.accent_text;
            }
            style
        }),
        pressed: Box::new(|focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.background = Some(Background::Color(p.accent));
            style.overlay = Some(Background::Color(Color::from_rgba8(0, 0, 0, 0.14)));
            style.text_color = Some(p.on_accent);
            style.icon_color = Some(p.on_accent);
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.accent_text;
            }
            style
        }),
    }
}

#[must_use]
pub fn secondary_button() -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(|focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.background = Some(Background::Color(p.card));
            style.border_width = 1.0;
            style.border_color = p.control_border;
            style.text_color = Some(p.text_primary);
            style.icon_color = Some(p.text_primary);
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.accent;
            }
            style
        }),
        disabled: Box::new(|theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.background = Some(Background::Color(p.recessed));
            style.border_width = 1.0;
            style.border_color = p.border;
            style.text_color = Some(p.text_tertiary);
            style.icon_color = Some(p.text_tertiary);
            style
        }),
        hovered: Box::new(|focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.background = Some(Background::Color(p.card));
            style.overlay = Some(Background::Color(p.hover_overlay));
            style.border_width = 1.0;
            style.border_color = p.control_border;
            style.text_color = Some(p.text_primary);
            style.icon_color = Some(p.text_primary);
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.accent;
            }
            style
        }),
        pressed: Box::new(|focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.background = Some(Background::Color(p.card));
            style.overlay = Some(Background::Color(p.pressed_overlay));
            style.border_width = 1.0;
            style.border_color = p.control_border;
            style.text_color = Some(p.text_primary);
            style.icon_color = Some(p.text_primary);
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.accent;
            }
            style
        }),
    }
}

#[must_use]
pub fn destructive_button() -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(|focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.background = Some(Background::Color(p.destructive_tint));
            style.border_width = 1.0;
            style.border_color = p.border;
            style.text_color = Some(p.destructive_text);
            style.icon_color = Some(p.destructive_text);
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.destructive;
            }
            style
        }),
        disabled: Box::new(|theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.background = Some(Background::Color(p.recessed));
            style.border_width = 1.0;
            style.border_color = p.border;
            style.text_color = Some(p.text_tertiary);
            style.icon_color = Some(p.text_tertiary);
            style
        }),
        hovered: Box::new(|focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.background = Some(Background::Color(p.destructive));
            style.text_color = Some(p.on_accent);
            style.icon_color = Some(p.on_accent);
            style.border_width = 1.0;
            style.border_color = p.destructive;
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.destructive;
            }
            style
        }),
        pressed: Box::new(|focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.background = Some(Background::Color(p.destructive));
            style.overlay = Some(Background::Color(Color::from_rgba8(0, 0, 0, 0.14)));
            style.text_color = Some(p.on_accent);
            style.icon_color = Some(p.on_accent);
            style.border_width = 1.0;
            style.border_color = p.destructive;
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.destructive;
            }
            style
        }),
    }
}

#[must_use]
pub fn card_button(selected: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_MD);
            style.background = Some(Background::Color(if selected {
                p.accent_tint
            } else {
                p.card
            }));
            style.border_width = if selected { 1.5 } else { 1.0 };
            style.border_color = if selected { p.accent } else { p.border };
            style.text_color = Some(p.text_primary);
            style.icon_color = Some(if selected {
                p.accent_text
            } else {
                p.text_secondary
            });
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.accent;
            }
            style
        }),
        disabled: Box::new(|theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_MD);
            style.background = Some(Background::Color(p.recessed));
            style.border_width = 1.0;
            style.border_color = p.border;
            style.text_color = Some(p.text_tertiary);
            style.icon_color = Some(p.text_tertiary);
            style
        }),
        hovered: Box::new(move |focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_MD);
            style.background = Some(Background::Color(if selected {
                p.accent_tint
            } else {
                p.card
            }));
            style.overlay = Some(Background::Color(p.hover_overlay));
            style.border_width = if selected { 1.5 } else { 1.0 };
            style.border_color = if selected { p.accent } else { p.control_border };
            style.text_color = Some(p.text_primary);
            style.icon_color = Some(if selected {
                p.accent_text
            } else {
                p.text_primary
            });
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.accent;
            }
            style
        }),
        pressed: Box::new(move |focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_MD);
            style.background = Some(Background::Color(if selected {
                p.accent_tint
            } else {
                p.card
            }));
            style.overlay = Some(Background::Color(p.pressed_overlay));
            style.border_width = if selected { 1.5 } else { 1.0 };
            style.border_color = if selected { p.accent } else { p.control_border };
            style.text_color = Some(p.text_primary);
            style.icon_color = Some(p.text_primary);
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.accent;
            }
            style
        }),
    }
}

#[must_use]
pub fn nav_pill_button(selected: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            if selected {
                style.background = Some(Background::Color(if p.is_dark {
                    p.selected_neutral
                } else {
                    p.card
                }));
                style.border_width = 1.0;
                style.border_color = p.border;
                style.text_color = Some(p.text_primary);
                style.icon_color = Some(p.accent_text);
            } else {
                style.background = None;
                style.text_color = Some(p.text_secondary);
                style.icon_color = Some(p.text_secondary);
            }
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.accent;
            }
            style
        }),
        disabled: Box::new(|theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.text_color = Some(p.text_tertiary);
            style.icon_color = Some(p.text_tertiary);
            style
        }),
        hovered: Box::new(move |focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.background = Some(Background::Color(if selected {
                if p.is_dark {
                    p.selected_neutral
                } else {
                    p.card
                }
            } else {
                p.hover_overlay
            }));
            if selected {
                style.border_width = 1.0;
                style.border_color = p.border;
            }
            style.text_color = Some(p.text_primary);
            style.icon_color = Some(if selected {
                p.accent_text
            } else {
                p.text_primary
            });
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.accent;
            }
            style
        }),
        pressed: Box::new(move |focused, theme| {
            let p = palette(theme);
            let mut style = base_button_style(RADIUS_SM);
            style.background = Some(Background::Color(p.pressed_overlay));
            style.text_color = Some(p.text_primary);
            style.icon_color = Some(p.text_primary);
            if focused {
                style.outline_width = 2.0;
                style.outline_color = p.accent;
            }
            style
        }),
    }
}

#[must_use]
pub fn apple_slider() -> cosmic::theme::iced::Slider {
    let build = |theme: &Theme, handle_border_width: f32, handle_border_color: Option<Color>| {
        let p = palette(theme);
        slider::Style {
            rail: slider::Rail {
                backgrounds: (
                    Background::Color(p.accent),
                    Background::Color(p.control_border),
                ),
                width: 4.0,
                border: Border {
                    radius: Radius::from(RADIUS_PILL),
                    color: Color::TRANSPARENT,
                    width: 0.0,
                },
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: 8.5 },
                background: Background::Color(Color::WHITE),
                border_width: handle_border_width,
                border_color: handle_border_color.unwrap_or(p.control_border),
            },
            breakpoint: slider::Breakpoint {
                color: p.text_tertiary,
            },
        }
    };
    cosmic::theme::iced::Slider::Custom {
        active: Rc::new(move |theme| build(theme, 1.5, None)),
        hovered: Rc::new(move |theme| {
            let p = palette(theme);
            build(theme, 2.0, Some(p.accent))
        }),
        dragging: Rc::new(move |theme| {
            let p = palette(theme);
            build(theme, 2.5, Some(p.accent))
        }),
    }
}

#[must_use]
pub fn apple_text_input() -> cosmic::theme::TextInput {
    let build = |theme: &Theme, border_width: f32, border_color: Color| {
        let p = palette(theme);
        text_input::Appearance {
            background: Background::Color(p.recessed),
            border_radius: Radius::from(RADIUS_SM),
            border_offset: None,
            border_width,
            border_color,
            label_color: p.text_primary,
            placeholder_color: p.text_tertiary,
            selected_text_color: p.on_accent,
            icon_color: Some(p.text_secondary),
            text_color: Some(p.text_primary),
            selected_fill: p.accent,
        }
    };
    cosmic::theme::TextInput::Custom {
        active: Box::new(move |theme| {
            let p = palette(theme);
            build(theme, 1.0, p.control_border)
        }),
        error: Box::new(move |theme| {
            let p = palette(theme);
            build(theme, 1.5, p.destructive)
        }),
        hovered: Box::new(move |theme| {
            let p = palette(theme);
            build(theme, 1.0, p.text_tertiary)
        }),
        focused: Box::new(move |theme| {
            let p = palette(theme);
            build(theme, 2.0, p.accent)
        }),
        disabled: Box::new(|theme| {
            let p = palette(theme);
            text_input::Appearance {
                background: Background::Color(p.recessed),
                border_radius: Radius::from(RADIUS_SM),
                border_offset: None,
                border_width: 1.0,
                border_color: p.border,
                label_color: p.text_tertiary,
                placeholder_color: p.text_tertiary,
                selected_text_color: p.on_accent,
                icon_color: Some(p.text_tertiary),
                text_color: Some(p.text_tertiary),
                selected_fill: p.accent,
            }
        }),
    }
}

fn primary_text_style(theme: &Theme) -> text::Style {
    let p = palette(theme);
    text::Style {
        color: Some(p.text_primary),
        selected_fill: p.accent,
        selected_text_color: Some(p.on_accent),
    }
}

fn secondary_text_style(theme: &Theme) -> text::Style {
    let p = palette(theme);
    text::Style {
        color: Some(p.text_secondary),
        selected_fill: p.accent,
        selected_text_color: Some(p.on_accent),
    }
}

fn tertiary_text_style(theme: &Theme) -> text::Style {
    let p = palette(theme);
    text::Style {
        color: Some(p.text_tertiary),
        selected_fill: p.accent,
        selected_text_color: Some(p.on_accent),
    }
}

pub fn page_title<'a>(
    content: impl Into<Cow<'a, str>> + 'a,
) -> widget::Text<'a, Theme, cosmic::Renderer> {
    widget::text::title2(content).class(cosmic::theme::Text::Custom(primary_text_style))
}

pub fn section_title<'a>(
    content: impl Into<Cow<'a, str>> + 'a,
) -> widget::Text<'a, Theme, cosmic::Renderer> {
    widget::text::caption_heading(content).class(cosmic::theme::Text::Custom(secondary_text_style))
}

pub fn row_title<'a>(
    content: impl Into<Cow<'a, str>> + 'a,
) -> widget::Text<'a, Theme, cosmic::Renderer> {
    widget::text::heading(content).class(cosmic::theme::Text::Custom(primary_text_style))
}

pub fn row_subtitle<'a>(
    content: impl Into<Cow<'a, str>> + 'a,
) -> widget::Text<'a, Theme, cosmic::Renderer> {
    widget::text::caption(content).class(cosmic::theme::Text::Custom(secondary_text_style))
}

pub fn muted_caption<'a>(
    content: impl Into<Cow<'a, str>> + 'a,
) -> widget::Text<'a, Theme, cosmic::Renderer> {
    widget::text::caption(content).class(cosmic::theme::Text::Custom(tertiary_text_style))
}

pub fn mono_value<'a>(
    content: impl Into<Cow<'a, str>> + 'a,
) -> widget::Text<'a, Theme, cosmic::Renderer> {
    widget::text::monotext(content).class(cosmic::theme::Text::Custom(secondary_text_style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::iced::widget::{
        container::Catalog as ContainerCatalog, rule::Catalog as RuleCatalog,
        slider::Catalog as SliderCatalog,
    };
    use cosmic::widget::{
        button::Catalog as ButtonCatalog, text_input::StyleSheet as TextInputStyleSheet,
    };

    #[test]
    fn light_and_dark_palettes_have_distinct_luminance() {
        let light = AppPalette::light();
        let dark = AppPalette::dark();
        assert!(!light.is_dark);
        assert!(dark.is_dark);
        assert!(light.canvas.relative_luminance() > 0.8);
        assert!(dark.canvas.relative_luminance() < 0.05);
        assert!(light.card.relative_luminance() > light.canvas.relative_luminance());
        assert!(dark.card.relative_luminance() > dark.canvas.relative_luminance());
    }

    #[test]
    fn light_and_dark_palettes_satisfy_wcag_aa_readability() {
        for p in [AppPalette::light(), AppPalette::dark()] {
            for bg in [p.canvas, p.card, p.recessed] {
                assert!(
                    p.text_primary.is_readable_on(bg),
                    "text_primary failed contrast on {bg:?} (dark={})",
                    p.is_dark
                );
                assert!(
                    p.text_secondary.is_readable_on(bg),
                    "text_secondary failed contrast on {bg:?} (dark={})",
                    p.is_dark
                );
                assert!(
                    p.text_tertiary.is_readable_on(bg),
                    "text_tertiary failed contrast on {bg:?} (dark={})",
                    p.is_dark
                );
            }
            assert!(
                p.on_accent.is_readable_on(p.accent),
                "on_accent failed contrast on accent (dark={})",
                p.is_dark
            );
            assert!(
                p.on_accent.is_readable_on(p.destructive),
                "on_accent failed contrast on destructive (dark={})",
                p.is_dark
            );
            assert!(
                p.warning_text.is_readable_on(p.warning_tint),
                "warning_text failed contrast on warning_tint (dark={})",
                p.is_dark
            );
            assert!(
                p.destructive_text.is_readable_on(p.destructive_tint),
                "destructive_text failed contrast on destructive_tint (dark={})",
                p.is_dark
            );
        }
    }

    #[test]
    fn style_builders_render_without_panicking_in_light_and_dark() {
        let light = Theme::light();
        let dark = Theme::dark();
        let light_palette = palette(&light);
        let dark_palette = palette(&dark);

        assert!(!light_palette.is_dark);
        assert!(dark_palette.is_dark);
        assert_ne!(
            canvas_container(&light).background,
            canvas_container(&dark).background
        );
        assert_ne!(
            card_container(&light).background,
            card_container(&dark).background
        );
        assert_ne!(
            sidebar_container(&light).background,
            sidebar_container(&dark).background
        );
        assert_ne!(hairline_rule(&light).color, hairline_rule(&dark).color);

        for theme in [&light, &dark] {
            let p = palette(theme);
            assert_eq!(
                canvas_container(theme).background,
                Some(Background::Color(p.canvas))
            );
            assert_eq!(
                card_container(theme).background,
                Some(Background::Color(p.card))
            );
            assert_eq!(
                sidebar_container(theme).background,
                Some(Background::Color(p.sidebar))
            );
            assert_eq!(hairline_rule(theme).color, p.divider);

            for (tone, expected_bg) in [
                (SemanticTone::Neutral, p.recessed),
                (SemanticTone::Accent, p.accent_tint),
                (SemanticTone::Warning, p.warning_tint),
            ] {
                assert_eq!(
                    pill_badge_container(tone)(theme).background,
                    Some(Background::Color(expected_bg))
                );
                let banner_bg = alert_banner_container(tone)(theme).background;
                assert!(banner_bg.is_some());
            }

            let primary = primary_button();
            assert_eq!(
                ButtonCatalog::active(theme, false, false, &primary).background,
                Some(Background::Color(p.accent))
            );
            assert_eq!(
                ButtonCatalog::active(theme, false, false, &primary).text_color,
                Some(p.on_accent)
            );
            assert!(
                ButtonCatalog::hovered(theme, false, false, &primary)
                    .overlay
                    .is_some()
            );
            assert!(
                ButtonCatalog::pressed(theme, false, false, &primary)
                    .overlay
                    .is_some()
            );
            assert_eq!(
                ButtonCatalog::disabled(theme, &primary).text_color,
                Some(p.text_tertiary)
            );

            for btn in [
                secondary_button(),
                destructive_button(),
                card_button(false),
                card_button(true),
                nav_pill_button(false),
                nav_pill_button(true),
            ] {
                assert!(
                    ButtonCatalog::active(theme, false, false, &btn)
                        .text_color
                        .is_some()
                );
                assert!(
                    ButtonCatalog::hovered(theme, false, false, &btn)
                        .text_color
                        .is_some()
                );
                assert!(
                    ButtonCatalog::pressed(theme, false, false, &btn)
                        .text_color
                        .is_some()
                );
                assert!(ButtonCatalog::disabled(theme, &btn).text_color.is_some());
            }

            let sl = apple_slider();
            assert_eq!(
                SliderCatalog::style(theme, &sl, slider::Status::Active)
                    .handle
                    .border_color,
                p.control_border
            );
            assert_eq!(
                SliderCatalog::style(theme, &sl, slider::Status::Hovered)
                    .handle
                    .border_color,
                p.accent
            );
            assert_eq!(
                SliderCatalog::style(theme, &sl, slider::Status::Dragged)
                    .handle
                    .border_color,
                p.accent
            );

            let ti = apple_text_input();
            assert_eq!(
                TextInputStyleSheet::active(theme, &ti).text_color,
                Some(p.text_primary)
            );
            assert_eq!(
                TextInputStyleSheet::hovered(theme, &ti).text_color,
                Some(p.text_primary)
            );
            assert_eq!(
                TextInputStyleSheet::focused(theme, &ti).border_color,
                p.accent
            );
            assert_eq!(
                TextInputStyleSheet::error(theme, &ti).border_color,
                p.destructive
            );
            assert_eq!(
                TextInputStyleSheet::disabled(theme, &ti).label_color,
                p.text_tertiary
            );

            let c = cosmic::theme::Container::custom(card_container);
            assert_eq!(
                ContainerCatalog::style(theme, &c).background,
                Some(Background::Color(p.card))
            );
            let r = cosmic::theme::Rule::custom(hairline_rule);
            assert_eq!(RuleCatalog::style(theme, &r).color, p.divider);
        }
    }
}
