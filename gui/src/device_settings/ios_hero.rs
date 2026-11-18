//! iOS 26 / iOS 27 AirPods Settings-inspired hero showcase, circular green battery rings,
//! Name pill card, and pill-shaped Listening Mode segmented control.

use std::borrow::Cow;
use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Instant;

use cosmic::{
    Apply, Element,
    iced::{Color, Length, Point, Radians, alignment, core::text::Wrapping, window},
    widget::{
        self,
        canvas::{self, path::Arc as PathArc},
    },
};
use openscq30_i18n::Translate;
use openscq30_lib::{
    DeviceModel,
    settings::{Select, Setting, SettingId},
};

use crate::{
    app::DebugOpenSCQ30Device,
    device_settings::device_3d::{self, Device3DModel},
    fl, fluid, icons,
    spring::{Spring, SpringParams},
    theme::{self, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS},
};

#[derive(Debug, Clone, Copy)]
struct BatteryRing {
    percent: u8,
    is_charging: bool,
}

struct BatteryRingState {
    initialized: bool,
    last_percent: u8,
    spring: Spring,
    last_tick: Option<Instant>,
}

impl Default for BatteryRingState {
    fn default() -> Self {
        Self {
            initialized: false,
            last_percent: 0,
            spring: Spring::new(0.0, SpringParams::GENTLE),
            last_tick: None,
        }
    }
}

impl<M> canvas::Program<M, cosmic::Theme, cosmic::Renderer> for BatteryRing {
    type State = BatteryRingState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        _bounds: cosmic::iced::Rectangle,
        _cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let target = f32::from(self.percent.min(100)) / 100.0;
        if !state.initialized {
            state.initialized = true;
            state.last_percent = self.percent;
            state.spring.snap_to(0.0);
            state.spring.set_target(target);
            state.last_tick = Some(Instant::now());
            return Some(canvas::Action::request_redraw());
        } else if state.last_percent != self.percent {
            state.last_percent = self.percent;
            state.spring.set_target(target);
            if state.last_tick.is_none() {
                state.last_tick = Some(Instant::now());
            }
            return Some(canvas::Action::request_redraw());
        }

        if let canvas::Event::Window(window::Event::RedrawRequested(now)) = event {
            let dt = state
                .last_tick
                .map_or(1.0 / 60.0, |prev| now.duration_since(prev).as_secs_f32());
            state.last_tick = Some(*now);
            if state.spring.step(dt) {
                return Some(canvas::Action::request_redraw());
            } else {
                state.last_tick = None;
            }
        } else if !state.spring.is_settled() && state.last_tick.is_none() {
            state.last_tick = Some(Instant::now());
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
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let is_dark = theme.cosmic().is_dark;

        let cx = bounds.width * 0.5;
        let cy = bounds.height * 0.5;
        let center = Point::new(cx, cy);
        let stroke_width = 3.6_f32;
        let radius = (bounds.width.min(bounds.height) * 0.5 - stroke_width).max(6.0);

        let track_color = if is_dark {
            Color::from_rgb8(58, 58, 60)
        } else {
            Color::from_rgb8(225, 228, 234)
        };
        let apple_green = Color::from_rgb8(52, 199, 89);
        let low_red = Color::from_rgb8(255, 59, 48);
        let ring_color = if self.percent <= 20 && !self.is_charging {
            low_red
        } else {
            apple_green
        };

        // Full circular background track
        let track_path = canvas::Path::new(|b| {
            b.arc(PathArc {
                center,
                radius,
                start_angle: Radians(0.0),
                end_angle: Radians(TAU),
            });
        });
        frame.stroke(
            &track_path,
            canvas::Stroke {
                style: canvas::stroke::Style::Solid(track_color),
                width: stroke_width,
                line_cap: canvas::LineCap::Round,
                line_join: canvas::LineJoin::Round,
                line_dash: canvas::LineDash::default(),
            },
        );

        // Progress arc starting from 12 o'clock (-PI/2)
        let clamped = if state.initialized {
            state.spring.value.clamp(0.0, 1.0)
        } else {
            f32::from(self.percent.min(100)) / 100.0
        };
        if clamped > 0.005 {
            let sweep = (clamped * TAU).min(TAU - 0.001);
            let arc_path = canvas::Path::new(|b| {
                b.arc(PathArc {
                    center,
                    radius,
                    start_angle: Radians(-FRAC_PI_2),
                    end_angle: Radians(-FRAC_PI_2 + sweep),
                });
            });
            frame.stroke(
                &arc_path,
                canvas::Stroke {
                    style: canvas::stroke::Style::Solid(ring_color),
                    width: stroke_width,
                    line_cap: canvas::LineCap::Round,
                    line_join: canvas::LineJoin::Round,
                    line_dash: canvas::LineDash::default(),
                },
            );
        }

        // Apple-style lightning bolt centered inside ring when charging
        if self.is_charging {
            let bolt = canvas::Path::new(|b| {
                let s = radius * 0.52;
                b.move_to(Point::new(cx + s * 0.14, cy - s * 0.95));
                b.line_to(Point::new(cx - s * 0.52, cy + s * 0.08));
                b.line_to(Point::new(cx + s * 0.02, cy + s * 0.08));
                b.line_to(Point::new(cx - s * 0.14, cy + s * 0.95));
                b.line_to(Point::new(cx + s * 0.52, cy - s * 0.08));
                b.line_to(Point::new(cx - s * 0.02, cy - s * 0.08));
                b.close();
            });
            frame.fill(&bolt, apple_green);
        }

        vec![frame.into_geometry()]
    }
}

fn parse_battery_fraction(raw: &str) -> Option<u8> {
    let trimmed = raw.trim();
    if let Some((num_s, den_s)) = trimmed.split_once('/') {
        let num: u32 = num_s.trim().parse().ok()?;
        let den: u32 = den_s.trim().parse().ok()?;
        if den == 0 {
            return None;
        }
        return Some(((num.saturating_mul(100)) / den).min(100) as u8);
    }
    if let Some(pct_s) = trimmed.strip_suffix('%') {
        return pct_s.trim().parse::<u8>().ok().map(|v| v.min(100));
    }
    trimmed.parse::<u8>().ok().map(|v| v.min(100))
}

fn read_percent(device: &DebugOpenSCQ30Device, id: SettingId) -> Option<u8> {
    match device.setting(&id)? {
        Setting::Information {
            value,
            translated_value,
        } => parse_battery_fraction(&value).or_else(|| parse_battery_fraction(&translated_value)),
        _ => None,
    }
}

fn read_charging(device: &DebugOpenSCQ30Device, id: SettingId) -> bool {
    match device.setting(&id) {
        Some(Setting::Information { value, .. }) => {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "true" | "charging" | "yes" | "1"
            )
        }
        Some(Setting::Toggle { value }) => value,
        _ => false,
    }
}

struct StudioShadow;

impl<M> canvas::Program<M, cosmic::Theme, cosmic::Renderer> for StudioShadow {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &cosmic::Renderer,
        theme: &cosmic::Theme,
        bounds: cosmic::iced::Rectangle,
        _cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Vec<canvas::Geometry<cosmic::iced::Renderer>> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let is_dark = theme.cosmic().is_dark;
        let cx = bounds.width * 0.5;
        let cy = bounds.height * 0.5;

        for (scale, alpha) in [(1.0_f32, 0.04), (0.76, 0.06), (0.50, 0.08)] {
            let rx = (bounds.width * 0.45) * scale;
            let ry = (bounds.height * 0.42) * scale;
            let path = canvas::Path::new(|b| {
                let steps = 28;
                for i in 0..=steps {
                    let a = (i as f32 / steps as f32) * TAU;
                    let pt = Point::new(cx + a.cos() * rx, cy + a.sin() * ry);
                    if i == 0 {
                        b.move_to(pt);
                    } else {
                        b.line_to(pt);
                    }
                }
                b.close();
            });
            let final_alpha = if is_dark { alpha * 2.2 } else { alpha };
            frame.fill(&path, Color::from_rgba(0.0, 0.0, 0.0, final_alpha));
        }
        vec![frame.into_geometry()]
    }
}

fn studio_shadow<'a, M: 'a>(width: f32) -> Element<'a, M> {
    widget::canvas(StudioShadow)
        .width(Length::Fixed(width))
        .height(Length::Fixed(12.0))
        .into()
}

fn battery_ring_column<'a, M: 'a>(
    percent: u8,
    is_charging: bool,
    caption: Option<&'a str>,
) -> Element<'a, M> {
    let ring_canvas: Element<'a, M> = widget::canvas(BatteryRing {
        percent,
        is_charging,
    })
    .width(Length::Fixed(38.0))
    .height(Length::Fixed(38.0))
    .into();

    let pct_text = widget::text::body(format!("{percent}%")).align_x(alignment::Horizontal::Center);

    widget::column![ring_canvas, pct_text]
        .push_maybe(caption.map(|c| theme::muted_caption(c).align_x(alignment::Horizontal::Center)))
        .spacing(4)
        .align_x(alignment::Horizontal::Center)
        .into()
}

fn mode_icon_for_option(option: &str) -> widget::icon::Handle {
    let lower = option.to_ascii_lowercase();
    if lower.contains("transparen") {
        icons::mode_transparency()
    } else if lower.contains("adaptive") {
        icons::mode_adaptive()
    } else if lower.contains("noise") || lower.contains("anc") {
        icons::mode_anc()
    } else {
        icons::mode_off()
    }
}

/// Renders an iOS 26 / iOS 27 fluid pill-shaped segmented control (`#E5E5EA` stadium track with
/// a spring-animated floating white pill, 1:1 drag/scrub, optional mode icons, and centered labels below).
pub fn ios_segmented_control<'a, M: Clone + 'static>(
    title: Option<Cow<'a, str>>,
    setting: &'a Select,
    selected_value: &str,
    show_mode_icons: bool,
    on_select: impl Fn(&str) -> M + Send + Sync + 'static,
) -> Element<'a, M> {
    let selected_idx = setting
        .options
        .iter()
        .position(|opt| opt.as_ref() == selected_value);
    let options = setting.options.clone();

    let mut segments: Vec<Element<'a, M>> = Vec::with_capacity(setting.options.len());
    let mut bottom_labels: Vec<Element<'a, M>> = Vec::with_capacity(setting.options.len());

    for (idx, localized) in setting.localized_options.iter().enumerate() {
        if show_mode_icons {
            let icon_handle = mode_icon_for_option(&options[idx]);
            let segment_el: Element<'a, M> = widget::container(widget::icon(icon_handle).size(22))
                .width(Length::Fill)
                .padding([9, 12])
                .align_x(alignment::Horizontal::Center)
                .align_y(alignment::Vertical::Center)
                .into();

            segments.push(segment_el);
            bottom_labels.push(
                widget::text::caption(localized.clone())
                    .wrapping(Wrapping::Word)
                    .align_x(alignment::Horizontal::Center)
                    .width(Length::FillPortion(1))
                    .into(),
            );
        } else {
            let segment_el: Element<'a, M> = widget::container(
                widget::text::caption_heading(localized.clone())
                    .wrapping(Wrapping::Word)
                    .align_x(alignment::Horizontal::Center)
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .padding([8, 12])
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .into();

            segments.push(segment_el);
        }
    }

    let track =
        fluid::fluid_segmented_control(segments, selected_idx, move |idx| on_select(&options[idx]));

    widget::column![]
        .push_maybe(title.map(|t| {
            Element::from(widget::container(theme::row_subtitle(t)).padding([0, SPACE_SM]))
        }))
        .push(track)
        .push_maybe(show_mode_icons.then(|| {
            Element::from(
                widget::row(bottom_labels)
                    .spacing(4)
                    .width(Length::Fill)
                    .padding([2, 4]),
            )
        }))
        .spacing(6)
        .width(Length::Fill)
        .into()
}

/// Renders the iOS 26 / iOS 27 AirPods / Q30 top hero showcase:
/// centered device title, sculpted product illustration, circular green battery rings,
/// and the `Name` summary pill card.
pub fn view_device_hero<'a, M: Clone + 'static>(
    device: &'a DebugOpenSCQ30Device,
) -> Element<'a, M> {
    let custom_name = match device.setting(&SettingId::DeviceName) {
        Some(Setting::Information {
            translated_value, ..
        }) if !translated_value.trim().is_empty() => Some(translated_value),
        _ => None,
    };
    let display_name = custom_name.unwrap_or_else(|| device.model().translate());

    let is_airpods = device.model() == DeviceModel::AppleAirPodsPro2;

    let showcase: Element<'a, M> = if is_airpods {
        let left_pct = read_percent(device, SettingId::BatteryLevelLeft);
        let right_pct = read_percent(device, SettingId::BatteryLevelRight);
        let left_chg = read_charging(device, SettingId::IsChargingLeft);
        let right_chg = read_charging(device, SettingId::IsChargingRight);
        let case_pct = read_percent(device, SettingId::CaseBatteryLevel);
        let case_chg = read_charging(device, SettingId::IsChargingCase);

        let buds_pct = match (left_pct, right_pct) {
            (Some(l), Some(r)) => Some(l.min(r)),
            (Some(l), None) => Some(l),
            (None, Some(r)) => Some(r),
            (None, None) => None,
        };
        let buds_chg = left_chg || right_chg;

        let buds_col = widget::column![
            device_3d::view_3d_device(Device3DModel::AirPodsBuds, 172.0, 122.0),
            studio_shadow(110.0),
        ]
        .push_maybe(buds_pct.map(|pct| battery_ring_column(pct, buds_chg, None)))
        .spacing(SPACE_SM)
        .align_x(alignment::Horizontal::Center);

        let case_col = widget::column![
            device_3d::view_3d_device(Device3DModel::AirPodsCase, 172.0, 122.0),
            studio_shadow(100.0),
        ]
        .push_maybe(case_pct.map(|pct| battery_ring_column(pct, case_chg, None)))
        .spacing(SPACE_SM)
        .align_x(alignment::Horizontal::Center);

        widget::row![buds_col, case_col]
            .spacing(36)
            .align_y(alignment::Vertical::Top)
            .into()
    } else {
        let single_pct = read_percent(device, SettingId::BatteryLevel);
        let single_chg = read_charging(device, SettingId::IsCharging);
        let left_pct = read_percent(device, SettingId::BatteryLevelLeft);
        let right_pct = read_percent(device, SettingId::BatteryLevelRight);
        let left_chg = read_charging(device, SettingId::IsChargingLeft);
        let right_chg = read_charging(device, SettingId::IsChargingRight);

        let battery_row: Option<Element<'a, M>> = if let Some(pct) = single_pct {
            Some(battery_ring_column(pct, single_chg, None))
        } else if left_pct.is_some() || right_pct.is_some() {
            Some(
                widget::row![]
                    .push_maybe(left_pct.map(|p| battery_ring_column(p, left_chg, Some("L"))))
                    .push_maybe(right_pct.map(|p| battery_ring_column(p, right_chg, Some("R"))))
                    .spacing(24)
                    .into(),
            )
        } else {
            None
        };

        widget::column![
            device_3d::view_3d_device(Device3DModel::SoundcoreQ30, 192.0, 132.0),
            studio_shadow(120.0),
        ]
        .push_maybe(battery_row)
        .spacing(SPACE_SM)
        .align_x(alignment::Horizontal::Center)
        .into()
    };

    let name_pill = widget::row![
        widget::text::body(fl!("name")).width(Length::Fill),
        theme::row_subtitle(display_name.clone()),
        theme::muted_caption("›"),
    ]
    .spacing(SPACE_XS)
    .align_y(alignment::Vertical::Center)
    .width(Length::Fill)
    .padding([14, SPACE_LG])
    .apply(widget::container)
    .class(cosmic::theme::Container::custom(theme::card_container))
    .width(Length::Fill);

    widget::column![
        widget::text::title3(display_name)
            .wrapping(Wrapping::Word)
            .align_x(alignment::Horizontal::Center),
        widget::container(showcase)
            .width(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .padding([SPACE_XS, 0]),
        name_pill,
    ]
    .spacing(SPACE_MD)
    .align_x(alignment::Horizontal::Center)
    .width(Length::Fill)
    .into()
}
