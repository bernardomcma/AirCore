use std::{collections::HashMap, sync::Arc};

use cosmic::{
    Apply, Element, Task,
    iced::{Length, alignment, core::text::Wrapping, keyboard},
    widget::{self, menu::KeyBind},
};
use itertools::Itertools;
use openscq30_i18n::Translate;
use openscq30_lib::{
    DeviceModel, OpenSCQ30Session, connection::ConnectionDescriptor, storage::PairedDevice,
};
use strum::IntoEnumIterator;
use tracing::error;

use crate::{
    components, fl,
    theme::{self, NARROW_BREAKPOINT, SPACE_LG, SPACE_MD, SPACE_SM},
};

pub const DEFAULT_SLIMMED_MODELS: [DeviceModel; 2] =
    [DeviceModel::SoundcoreA3028, DeviceModel::AppleAirPodsPro2];

pub struct AddDeviceModel {
    stage: Stage,
    session: Arc<OpenSCQ30Session>,
    key_binds: HashMap<KeyBind, KeyBindAction>,
}

enum Stage {
    ModelSelection(ModelSelectionModel),
    SelectDevice(SelectDeviceModel),
    Error(String),
}

pub struct ModelSearchEntry {
    pub model: DeviceModel,
    pub raw_lowercase: String,
    pub localized_lowercase: String,
}

struct ModelSelectionModel {
    search_query: String,
    search_widget_id: widget::Id,
    show_all_models: bool,
    entries: Vec<ModelSearchEntry>,
}

struct SelectDeviceModel {
    search_query: String,
    search_widget_id: widget::Id,
    devices: Option<Vec<ConnectionDescriptor>>,
    device_model: DeviceModel,
    is_demo_mode: bool,
}

#[derive(Clone, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum Message {
    SetDeviceModelSearchQuery(String),
    ToggleShowAllModels(bool),
    SelectModel(DeviceModel, bool),
    SelectDevice(usize, bool),
    SetDeviceList(Vec<ConnectionDescriptor>, bool),
    SetDeviceNameSearchQuery(String),
    SetErrorMessage(String),
}

pub enum Action {
    None,
    Task(Task<Message>),
    AddDevice(PairedDevice),
    FocusTextInput(cosmic::widget::Id),
}

pub fn build_model_search_entries() -> Vec<ModelSearchEntry> {
    DeviceModel::iter()
        .map(|model| ModelSearchEntry {
            model,
            raw_lowercase: <&'static str>::from(model).to_lowercase(),
            localized_lowercase: model.translate().to_lowercase(),
        })
        .collect()
}

pub fn filter_and_sort_models(
    entries: &[ModelSearchEntry],
    search_query: &str,
    show_all_models: bool,
) -> Vec<DeviceModel> {
    let search_query_lowercase = search_query.to_lowercase();
    entries
        .iter()
        .filter(|entry| show_all_models || DEFAULT_SLIMMED_MODELS.contains(&entry.model))
        .filter_map(|entry| {
            let maybe_index = entry.localized_lowercase.find(&search_query_lowercase);
            // Only search by model name if the search query is long enough to prevent including,
            // for example, all a30* devices when someone searches "30" to find "Soundcore Q30"
            if maybe_index.is_some()
                || (search_query_lowercase.len() >= 4
                    && entry.raw_lowercase.contains(&search_query_lowercase))
            {
                // put model_name matches after localized_name matches. we use a stable sort,
                // so giving them all usize::MAX won't affect the ordering
                Some((entry.model, maybe_index.unwrap_or(usize::MAX)))
            } else {
                None
            }
        })
        .sorted_by(|(_, left), (_, right)| left.cmp(right))
        .map(|(device_model, _)| device_model)
        .collect()
}

pub fn filter_connection_descriptors<'a>(
    devices: &'a [ConnectionDescriptor],
    search_query: &str,
) -> Vec<(usize, &'a ConnectionDescriptor)> {
    let query_lower = search_query.to_lowercase();
    devices
        .iter()
        .enumerate()
        .filter(|(_, device)| device.name.to_lowercase().contains(&query_lower))
        .collect()
}

impl AddDeviceModel {
    pub fn new(session: Arc<OpenSCQ30Session>) -> Self {
        Self {
            session,
            stage: Stage::ModelSelection(ModelSelectionModel {
                search_query: String::new(),
                search_widget_id: widget::Id::unique(),
                show_all_models: false,
                entries: build_model_search_entries(),
            }),
            key_binds: key_binds(),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        match &self.stage {
            Stage::ModelSelection(ui_model) => Self::device_model_selection(ui_model),
            Stage::SelectDevice(ui_model) => Self::select_device(ui_model),
            Stage::Error(message) => Self::error(message),
        }
    }

    fn device_model_selection(ui_model: &ModelSelectionModel) -> Element<'_, Message> {
        let filtered_models = filter_and_sort_models(
            &ui_model.entries,
            &ui_model.search_query,
            ui_model.show_all_models,
        );

        widget::responsive(move |size| {
            let is_narrow = size.width < NARROW_BREAKPOINT;

            let header = components::page_header(
                fl!("select-device-model"),
                Some(fl!("select-device-model-subtitle").into()),
                None,
            );

            let search_input = widget::search_input(fl!("device-model"), &ui_model.search_query)
                .id(ui_model.search_widget_id.clone())
                .style(theme::apple_text_input())
                .on_input(Message::SetDeviceModelSearchQuery);

            let show_all_toggle = widget::toggler(ui_model.show_all_models)
                .label(fl!("show-all-models"))
                .spacing(SPACE_SM)
                .on_toggle(Message::ToggleShowAllModels);

            let toolbar: Element<'_, Message> = if is_narrow {
                widget::column![search_input, show_all_toggle]
                    .spacing(SPACE_SM)
                    .width(Length::Fill)
                    .into()
            } else {
                widget::row![search_input.width(Length::Fill), show_all_toggle]
                    .spacing(SPACE_MD)
                    .align_y(alignment::Vertical::Center)
                    .width(Length::Fill)
                    .into()
            };

            let list_content: Element<'_, Message> = if filtered_models.is_empty() {
                components::empty_state(
                    crate::icons::openscq30(),
                    fl!("no-matching-models"),
                    fl!("select-device-model-subtitle"),
                    None,
                )
            } else {
                widget::column(filtered_models.iter().copied().map(|device_model| {
                    let display_name = device_model.translate();
                    widget::button::custom(
                        widget::column![
                            theme::row_title(display_name.clone()),
                            theme::mono_value(device_model.to_string()),
                        ]
                        .spacing(2)
                        .width(Length::Fill)
                        .padding([SPACE_MD, SPACE_LG]),
                    )
                    .name(display_name)
                    .class(theme::card_button(false))
                    .width(Length::Fill)
                    .on_press(Message::SelectModel(device_model, false))
                    .into()
                }))
                .spacing(SPACE_SM)
                .width(Length::Fill)
                .into()
            };

            widget::column![
                components::page_frame(
                    widget::column![header, toolbar]
                        .spacing(SPACE_MD)
                        .width(Length::Fill),
                ),
                widget::scrollable(components::page_frame(list_content))
                    .width(Length::Fill)
                    .height(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        })
        .into()
    }

    fn select_device(ui_model: &SelectDeviceModel) -> Element<'_, Message> {
        widget::responsive(|size| {
            let is_narrow = size.width < NARROW_BREAKPOINT;

            let header = components::page_header(
                fl!("select-your", name = ui_model.device_model.translate()),
                None,
                None,
            );

            let search_input = widget::search_input(fl!("device-name"), &ui_model.search_query)
                .id(ui_model.search_widget_id.clone())
                .style(theme::apple_text_input())
                .on_input(Message::SetDeviceNameSearchQuery);

            let demo_toggle = widget::toggler(ui_model.is_demo_mode)
                .label(fl!("demo-mode"))
                .spacing(SPACE_SM)
                .on_toggle(|enabled| Message::SelectModel(ui_model.device_model, enabled));

            let refresh = widget::button::standard(fl!("refresh"))
                .leading_icon(widget::icon::from_name("view-refresh-symbolic"))
                .on_press(Message::SelectModel(
                    ui_model.device_model,
                    ui_model.is_demo_mode,
                ));

            let toolbar: Element<'_, Message> = if is_narrow {
                widget::column![
                    search_input,
                    widget::row![
                        demo_toggle.apply(widget::container).width(Length::Fill),
                        refresh
                            .apply(widget::container)
                            .width(Length::Fill)
                            .align_x(alignment::Horizontal::Right),
                    ]
                    .spacing(SPACE_SM)
                    .align_y(alignment::Vertical::Center),
                ]
                .spacing(SPACE_SM)
                .into()
            } else {
                widget::row![search_input, demo_toggle, refresh]
                    .align_y(alignment::Vertical::Center)
                    .spacing(SPACE_MD)
                    .into()
            };

            let list_body: Element<'_, Message> = if let Some(devices) = &ui_model.devices {
                let matched = filter_connection_descriptors(devices, &ui_model.search_query);
                if matched.is_empty() {
                    components::empty_state(
                        crate::icons::openscq30(),
                        fl!("no-matching-devices"),
                        fl!("no-matching-devices-hint"),
                        None,
                    )
                } else {
                    widget::column(matched.into_iter().map(|(index, device)| {
                        widget::button::custom(
                            widget::row![
                                theme::row_title(&device.name).width(Length::Fill),
                                theme::mono_value(device.mac_address.to_string())
                                    .align_x(alignment::Horizontal::Right),
                            ]
                            .spacing(SPACE_MD)
                            .align_y(alignment::Vertical::Center)
                            .padding([SPACE_MD, SPACE_LG]),
                        )
                        .name(&device.name)
                        .class(theme::card_button(false))
                        .width(Length::Fill)
                        .on_press(Message::SelectDevice(index, ui_model.is_demo_mode))
                        .into()
                    }))
                    .spacing(SPACE_SM)
                    .width(Length::Fill)
                    .into()
                }
            } else {
                Self::loading()
            };

            widget::column![
                components::page_frame(
                    widget::column![header, toolbar]
                        .spacing(SPACE_MD)
                        .width(Length::Fill),
                ),
                if ui_model.devices.is_some() {
                    widget::scrollable(components::page_frame(list_body))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                } else {
                    list_body
                },
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        })
        .into()
    }

    fn loading() -> Element<'static, Message> {
        widget::progress_bar::indeterminate_circular()
            .size(44.0)
            .apply(widget::container)
            .center(Length::Fill)
            .into()
    }

    fn error(message: &str) -> Element<'_, Message> {
        components::page_frame(
            components::elevated_card(
                widget::column![
                    widget::text::title3(fl!("error-loading-devices")),
                    widget::text::monotext(message).wrapping(Wrapping::WordOrGlyph),
                ]
                .spacing(SPACE_SM)
                .width(Length::Fill),
            )
            .max_width(560.0)
            .apply(widget::container)
            .center(Length::Fill),
        )
    }

    #[must_use]
    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::SetDeviceModelSearchQuery(query) => {
                if let Stage::ModelSelection(ref mut ui_model) = self.stage {
                    ui_model.search_query = query;
                }
            }
            Message::ToggleShowAllModels(show_all) => {
                if let Stage::ModelSelection(ref mut ui_model) = self.stage {
                    ui_model.show_all_models = show_all;
                }
            }
            Message::SelectModel(device_model, is_demo_mode) => {
                if let Stage::SelectDevice(ref mut ui_model) = self.stage {
                    // if we're already on the select device page, don't clear the search and such
                    ui_model.device_model = device_model;
                    ui_model.is_demo_mode = is_demo_mode;
                    ui_model.devices = None;
                } else {
                    self.stage = Stage::SelectDevice(SelectDeviceModel {
                        search_query: String::new(),
                        search_widget_id: widget::Id::unique(),
                        devices: None,
                        device_model,
                        is_demo_mode,
                    });
                }
                let session = self.session.clone();
                return Action::Task(Task::perform(
                    async move {
                        if is_demo_mode {
                            session.list_demo_devices(device_model).await
                        } else {
                            session.list_devices(device_model).await
                        }
                    },
                    move |result| match result {
                        Ok(devices) => Message::SetDeviceList(devices, is_demo_mode),
                        Err(err) => {
                            error!("{} fetching devices: {err:?}", device_model);
                            Message::SetErrorMessage(format!("{err}"))
                        }
                    },
                ));
            }
            Message::SetDeviceList(devices, is_demo_mode) => {
                if let Stage::SelectDevice(ref mut ui_model) = self.stage {
                    ui_model.devices = Some(devices);
                    ui_model.is_demo_mode = is_demo_mode;
                }
            }
            Message::SetDeviceNameSearchQuery(query) => {
                if let Stage::SelectDevice(ref mut ui_model) = self.stage {
                    ui_model.search_query = query;
                }
            }
            Message::SetErrorMessage(message) => self.stage = Stage::Error(message),
            Message::SelectDevice(index, is_demo) => {
                if let Stage::SelectDevice(ui_model) = &self.stage
                    && let Some(devices) = &ui_model.devices
                    && let Some(descriptor) = devices.get(index)
                {
                    return Action::AddDevice(PairedDevice {
                        mac_address: descriptor.mac_address,
                        model: ui_model.device_model,
                        is_demo,
                    });
                }
            }
        }
        Action::None
    }

    #[must_use]
    pub fn on_key_pressed(
        &mut self,
        modifiers: keyboard::Modifiers,
        key: &keyboard::Key,
        physical_key: &keyboard::key::Physical,
    ) -> Action {
        let action = self
            .key_binds
            .iter()
            .find(|(bind, _)| bind.matches(modifiers, key, Some(physical_key)))
            .map(|(_, action)| action)
            .copied();
        if let Some(action) = action {
            self.handle_key_bind_action(action)
        } else {
            Action::None
        }
    }

    fn handle_key_bind_action(&mut self, action: KeyBindAction) -> Action {
        match action {
            KeyBindAction::Find => match &self.stage {
                Stage::ModelSelection(model_selection) => {
                    Action::FocusTextInput(model_selection.search_widget_id.clone())
                }
                Stage::SelectDevice(select_device) => {
                    Action::FocusTextInput(select_device.search_widget_id.clone())
                }
                Stage::Error(_) => Action::None,
            },
        }
    }
}

#[derive(Clone, Copy)]
enum KeyBindAction {
    Find,
}

fn key_binds() -> HashMap<KeyBind, KeyBindAction> {
    let mut key_binds = HashMap::new();

    key_binds.insert(
        KeyBind {
            modifiers: vec![cosmic::widget::menu::key_bind::Modifier::Ctrl],
            key: keyboard::Key::Character("f".into()),
        },
        KeyBindAction::Find,
    );

    key_binds
}

#[cfg(test)]
mod tests {
    use macaddr::MacAddr6;
    use openscq30_lib::DeviceModel;

    use super::*;

    #[test]
    fn default_slimmed_list_is_q30_and_airpods_pro_2() {
        let entries = build_model_search_entries();
        let slimmed = filter_and_sort_models(&entries, "", false);
        assert_eq!(
            slimmed,
            vec![DeviceModel::SoundcoreA3028, DeviceModel::AppleAirPodsPro2]
        );
    }

    #[test]
    fn show_all_models_expands_to_full_enum_catalog() {
        let entries = build_model_search_entries();
        let all = filter_and_sort_models(&entries, "", true);
        assert_eq!(all.len(), entries.len());
        assert!(all.contains(&DeviceModel::SoundcoreA3040));
    }

    #[test]
    fn raw_model_id_search_requires_at_least_4_chars() {
        let entries = build_model_search_entries();
        let short_match = filter_and_sort_models(&entries, "a30", false);
        assert!(!short_match.contains(&DeviceModel::SoundcoreA3028));

        let full_match = filter_and_sort_models(&entries, "a302", false);
        assert_eq!(full_match, vec![DeviceModel::SoundcoreA3028]);
    }

    #[test]
    fn special_char_search_returns_empty() {
        let entries = build_model_search_entries();
        assert!(filter_and_sort_models(&entries, "!@#$%_none", false).is_empty());
        assert!(filter_and_sort_models(&entries, "!@#$%_none", true).is_empty());
    }

    #[test]
    fn filter_connection_descriptors_preserves_original_indices() {
        let devices = vec![
            ConnectionDescriptor {
                name: "Life Q30".to_string(),
                mac_address: MacAddr6::from([0x11, 0x22, 0x33, 0x44, 0x55, 0x66]),
            },
            ConnectionDescriptor {
                name: "AirPods Pro 2".to_string(),
                mac_address: MacAddr6::from([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]),
            },
        ];
        let filtered = filter_connection_descriptors(&devices, "airpods");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].0, 1);
        assert_eq!(filtered[0].1.name, "AirPods Pro 2");
    }

    #[tokio::test]
    async fn select_device_handles_valid_and_out_of_bounds_index_safely() {
        let config_dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            OpenSCQ30Session::new(config_dir.path().join("database.sqlite"))
                .await
                .unwrap(),
        );
        let mut model = AddDeviceModel::new(session);
        let mac = MacAddr6::from([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let _ = model.update(Message::SelectModel(DeviceModel::AppleAirPodsPro2, true));
        let _ = model.update(Message::SetDeviceList(
            vec![ConnectionDescriptor {
                name: "AirPods Pro 2".to_string(),
                mac_address: mac,
            }],
            true,
        ));

        assert!(matches!(
            model.update(Message::SelectDevice(99, true)),
            Action::None
        ));
        assert!(matches!(
            model.update(Message::SelectDevice(0, true)),
            Action::AddDevice(paired)
                if paired.mac_address == mac
                    && paired.model == DeviceModel::AppleAirPodsPro2
                    && paired.is_demo
        ));
    }
}
