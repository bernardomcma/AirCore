use std::collections::HashMap;

use cosmic::{
    Apply, Element,
    iced::{
        Length,
        alignment::{Horizontal, Vertical},
        core::text::Wrapping,
    },
    widget,
};

use crate::{
    components,
    device_settings::eq_helpers::format_db_list,
    equalizer_line::EqualizerLine,
    fl,
    openscq30_v1_migration::LegacyEqualizerProfile,
    theme::{self, SPACE_LG, SPACE_MD, SPACE_SM},
};

struct LegacyProfileInfo {
    name: String,
    values: Vec<i16>,
    visualization: EqualizerLine<Message>,
}

pub struct LegacyMigrationModel {
    profiles: Vec<LegacyProfileInfo>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Migrate(String, Vec<i16>),
}

impl LegacyMigrationModel {
    pub fn new(profiles: HashMap<String, LegacyEqualizerProfile>) -> Self {
        let mut profiles = profiles
            .into_iter()
            .map(|(name, profile)| LegacyProfileInfo {
                name: name.to_owned(),
                values: profile.volume_offsets.to_owned(),
                visualization: EqualizerLine::new(-120, 135, profile.volume_offsets),
            })
            .collect::<Vec<_>>();
        profiles.sort_unstable_by(|a, b| a.name.cmp(&b.name));

        Self { profiles }
    }

    pub fn view(&self, is_narrow: bool) -> Element<'_, Message> {
        let header = components::page_header(fl!("legacy-equalizer-profile-migration"), None, None);

        let cards = widget::column(self.profiles.iter().map(|profile| {
            let curve_block = widget::column![
                theme::row_title(&profile.name).wrapping(Wrapping::WordOrGlyph),
                theme::mono_value(format_db_list(&profile.values, 1)),
                widget::responsive(|size| {
                    profile
                        .visualization
                        .view()
                        .apply(widget::container)
                        .width(size.width)
                        .height(40)
                        .into()
                })
                .apply(widget::container)
                // responsive wants to fill all available height, but that is not desirable
                // it will even crash due to being a child of scrollable, so constrain its height
                // by wrapping in a container
                .height(40),
            ]
            .spacing(SPACE_SM)
            .align_x(Horizontal::Left)
            .width(Length::Fill);

            let migrate_button = components::primary_button(
                fl!("migrate"),
                Message::Migrate(profile.name.to_owned(), profile.values.to_owned()),
            );

            let card_body: Element<'_, Message> = if is_narrow {
                widget::column![
                    curve_block,
                    widget::container(migrate_button)
                        .width(Length::Fill)
                        .align_x(Horizontal::Right),
                ]
                .spacing(SPACE_MD)
                .width(Length::Fill)
                .into()
            } else {
                widget::row![curve_block, migrate_button]
                    .spacing(SPACE_LG)
                    .align_y(Vertical::Center)
                    .width(Length::Fill)
                    .into()
            };

            components::elevated_card(card_body).into()
        }))
        .spacing(SPACE_MD)
        .width(Length::Fill);

        widget::scrollable(components::page_frame(
            widget::column![header, cards]
                .spacing(SPACE_LG)
                .width(Length::Fill),
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
