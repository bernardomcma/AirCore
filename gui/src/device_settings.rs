mod controls;
pub mod eq_helpers;
pub mod ios_hero;
mod legacy_migration;
mod quick_presets;

use std::{
    borrow::Cow,
    collections::{HashMap, hash_map},
    path::PathBuf,
};

use cosmic::{
    Apply, Element, Task,
    app::context_drawer::ContextDrawer,
    iced::{Length, alignment, core::text::Wrapping, keyboard},
    widget::{self, nav_bar},
};
use legacy_migration::LegacyMigrationModel;
use openscq30_i18n::Translate;
use openscq30_lib::{
    connection::ConnectionStatus,
    quick_presets::QuickPresetsHandler,
    settings::{self, CategoryId, Setting, SettingId, Value},
};
use tracing::{Instrument, debug};

use crate::{
    app::DebugOpenSCQ30Device,
    components,
    device_settings::eq_helpers::{
        PresetPreviewData, build_preset_previews, format_db_value, format_hz_label,
    },
    equalizer_line::EqualizerLine,
    fl, fluid, handle_soft_error,
    openscq30_v1_migration::{self, LegacyEqualizerProfile},
    theme::{
        self, EQ_TWO_COLUMN_BREAKPOINT, NARROW_BREAKPOINT, SIDEBAR_BREAKPOINT, SPACE_LG, SPACE_MD,
        SPACE_SM, SPACE_XS, SemanticTone,
    },
    throttle,
    utils::coalesce_result,
};

pub const MODIFIABLE_SELECT_ADD_INPUT_ID: &str = "modifiable-select-dialog-add-item-text-input";

#[derive(Debug, Clone)]
pub enum Message {
    SelectCategory(nav_bar::Id),
    QuickPresets(quick_presets::Message),
    Throttle(throttle::Message),
    SetSetting(SettingId, Value),
    SetEqualizerBand(SettingId, u8, i16),
    RefreshSettings,
    Warning(String),
    CancelDialog,
    ShowModifiableSelectAddDialog(SettingId),
    ShowModifiableSelectRemoveDialog(SettingId),
    ModifiableSelectAddDialogSubmit(Option<String>),
    ModifiableSelectAddDialogSetName(String),
    ModifiableSelectRemoveDialogSubmit,
    AddLegacyEqualizerMigrationPage(HashMap<String, LegacyEqualizerProfile>),
    LegacyMigration(legacy_migration::Message),
    None,
    CopyToClipboard(String),
    SetImportString(SettingId, String),
    AskConfirmImportString(SettingId, String),
    ConfirmImportString,
    Disconnect,
    SetTimeTwelveHour(SettingId, String),
    SetTimeMinute(SettingId, String),
    SetTimePm(SettingId, bool),
    RefreshTimeTwelveHour(SettingId),
}

impl From<quick_presets::Message> for Message {
    fn from(message: quick_presets::Message) -> Self {
        Self::QuickPresets(message)
    }
}

impl From<throttle::Message> for Message {
    fn from(message: throttle::Message) -> Self {
        Self::Throttle(message)
    }
}

#[must_use]
pub enum Action {
    Task(Task<Message>),
    Warning(String),
    FocusTextInput(widget::Id),
    None,
    Disconnect,
}

pub struct DeviceSettingsModel {
    device: DebugOpenSCQ30Device,
    nav_model: nav_bar::Model,
    settings_order: Vec<SettingId>,
    settings: HashMap<SettingId, SettingUiState>,
    dialog: Option<Dialog>,
    legacy_equalizer_migration: Option<legacy_migration::LegacyMigrationModel>,
    quick_presets_model: quick_presets::QuickPresetsModel,
    throttle: throttle::Throttle,
}

#[derive(Debug)]
struct SettingUiState {
    setting_id: SettingId,
    translated_name: String,
    variant_state: SettingVariantUiState,
}

#[derive(Debug)]
enum SettingVariantUiState {
    Toggle {
        value: bool,
    },
    I32Range {
        setting: settings::Range<i32>,
        value: i32,
    },
    Select {
        setting: settings::Select,
        value: Cow<'static, str>,
    },
    OptionalSelect {
        setting: settings::Select,
        value: Option<Cow<'static, str>>,
    },
    ModifiableSelect {
        setting: settings::Select,
        value: Option<Cow<'static, str>>,
    },
    MultiSelect {
        setting: settings::Select,
        values: Vec<Cow<'static, str>>,
    },
    MultiSelectWithRemove {
        setting: settings::Select,
        values: Vec<Cow<'static, str>>,
    },
    Equalizer {
        setting: settings::Equalizer,
        read_only: bool,
        value: Vec<i16>,
        line: EqualizerLine<Message>,
    },
    PresetEqualizerProfileSelect {
        equalizer: settings::Equalizer,
        select: settings::Select,
        presets: Vec<Vec<i16>>,
        value: Option<Cow<'static, str>>,
        previews: Vec<(PresetPreviewData, EqualizerLine<Message>)>,
    },
    Information {
        translated_value: String,
    },
    ImportString {
        text: String,
        confirmation_message: Option<String>,
    },
    HueColorPicker {
        /// in degrees, 0 to 360
        hue: f32,
    },
    Action,
    TimeOfDay {
        minutes_after_midnight: MinutesAfterMidnight,
        hour_text: Option<String>,
        minute_text: Option<String>,
    },
}

fn build_preview_lines(
    equalizer: &settings::Equalizer,
    select: &settings::Select,
    presets: &[Vec<i16>],
    value: Option<&str>,
) -> Vec<(PresetPreviewData, EqualizerLine<Message>)> {
    build_preset_previews(equalizer, select, presets, value)
        .into_iter()
        .map(|preview| {
            let line = EqualizerLine::thumbnail(
                equalizer.min,
                equalizer.max,
                preview.values.clone(),
                preview.is_active,
            );
            (preview, line)
        })
        .collect()
}

impl From<Setting> for SettingVariantUiState {
    fn from(setting: Setting) -> Self {
        match setting {
            Setting::Toggle { value } => Self::Toggle { value },
            Setting::I32Range { setting, value } => Self::I32Range { setting, value },
            Setting::Select { setting, value } => Self::Select { setting, value },
            Setting::OptionalSelect { setting, value } => Self::OptionalSelect { setting, value },
            Setting::ModifiableSelect { setting, value } => {
                Self::ModifiableSelect { setting, value }
            }
            Setting::MultiSelect { setting, values } => Self::MultiSelect { setting, values },
            Setting::MultiSelectWithRemove { setting, values } => {
                Self::MultiSelectWithRemove { setting, values }
            }
            Setting::Equalizer {
                setting,
                read_only,
                value,
            } => {
                let line = EqualizerLine::new(setting.min, setting.max, value.clone());
                Self::Equalizer {
                    setting,
                    read_only,
                    value,
                    line,
                }
            }
            Setting::PresetEqualizerProfileSelect {
                equalizer,
                select,
                presets,
                value,
            } => {
                let previews = build_preview_lines(&equalizer, &select, &presets, value.as_deref());
                Self::PresetEqualizerProfileSelect {
                    equalizer,
                    select,
                    presets,
                    value,
                    previews,
                }
            }
            Setting::Information {
                value: _,
                translated_value,
            } => Self::Information { translated_value },
            Setting::ImportString {
                confirmation_message,
            } => Self::ImportString {
                text: String::new(),
                confirmation_message,
            },
            Setting::HueColorPicker { hue } => Self::HueColorPicker { hue },
            Setting::Action => Self::Action,
            Setting::TimeOfDay {
                minutes_after_midnight,
            } => Self::TimeOfDay {
                minutes_after_midnight: MinutesAfterMidnight(minutes_after_midnight),
                hour_text: None,
                minute_text: None,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct MinutesAfterMidnight(i32);

impl MinutesAfterMidnight {
    fn twenty_four_hour(self) -> i32 {
        self.0 / 60
    }

    fn twelve_hour(self) -> i32 {
        let twelve_hour_from_zero = self.twenty_four_hour() % 12;
        if twelve_hour_from_zero == 0 {
            12
        } else {
            twelve_hour_from_zero
        }
    }

    fn minute(self) -> i32 {
        self.0 % 60
    }

    fn with_twenty_four_hour(self, twenty_four_hour: i32) -> Self {
        let twenty_four_hour = twenty_four_hour.clamp(0, 23);
        Self(twenty_four_hour * 60 + self.minute())
    }

    fn with_twelve_hour(self, twelve_hour: i32) -> Self {
        let twelve_hour = twelve_hour.clamp(1, 12);
        let twelve_hour_from_zero = if twelve_hour == 12 { 0 } else { twelve_hour };
        let pm_offset = if self.is_pm() { 12 * 60 } else { 0 };
        Self(pm_offset + twelve_hour_from_zero * 60 + self.minute())
    }

    fn with_minute(self, minute: i32) -> Self {
        let minute = minute.clamp(0, 59);
        Self(self.twenty_four_hour() * 60 + minute)
    }

    fn is_pm(self) -> bool {
        self.twenty_four_hour() >= 12
    }

    fn with_pm(self, is_pm: bool) -> Self {
        if is_pm && !self.is_pm() {
            self.with_twenty_four_hour(self.twenty_four_hour() + 12)
        } else if !is_pm && self.is_pm() {
            self.with_twenty_four_hour(self.twenty_four_hour() - 12)
        } else {
            self
        }
    }
}

impl SettingVariantUiState {
    fn update(&mut self, setting: Setting) {
        // We only need to cover the cases where self and setting are the same variant and there is
        // state that will not be reconstructed by Self::from.
        // For the variant mismatch case, we can throw away any variant specific state.
        match (self, setting) {
            (
                Self::ImportString {
                    text: _,
                    confirmation_message,
                },
                Setting::ImportString {
                    confirmation_message: new_confirmation_message,
                },
            ) => {
                *confirmation_message = new_confirmation_message;
            }
            (
                Self::TimeOfDay {
                    minutes_after_midnight,
                    ..
                },
                Setting::TimeOfDay {
                    minutes_after_midnight: new_minutes_after_midnight,
                },
            ) if minutes_after_midnight.0 == new_minutes_after_midnight => {
                // Don't reset text inputs if the time hasn't changed
            }
            (
                Self::Equalizer {
                    setting,
                    read_only,
                    value,
                    line,
                },
                Setting::Equalizer {
                    setting: new_setting,
                    read_only: new_read_only,
                    value: new_value,
                },
            ) => {
                *setting = new_setting;
                *read_only = new_read_only;
                line.set_values(setting.min, setting.max, new_value.clone());
                *value = new_value;
            }
            (
                Self::PresetEqualizerProfileSelect {
                    equalizer,
                    select,
                    presets,
                    value,
                    previews,
                },
                Setting::PresetEqualizerProfileSelect {
                    equalizer: new_equalizer,
                    select: new_select,
                    presets: new_presets,
                    value: new_value,
                },
            ) => {
                if *equalizer != new_equalizer
                    || *select != new_select
                    || *presets != new_presets
                    || *value != new_value
                {
                    *previews = build_preview_lines(
                        &new_equalizer,
                        &new_select,
                        &new_presets,
                        new_value.as_deref(),
                    );
                    *equalizer = new_equalizer;
                    *select = new_select;
                    *presets = new_presets;
                    *value = new_value;
                }
            }
            (this, setting) => *this = setting.into(),
        }
    }
}

enum Dialog {
    ModifiableSelectAdd(SettingId, String),
    ModifiableSelectRemove(SettingId, Cow<'static, str>),
    ImportStringConfirm(SettingId, String),
}

enum CustomCategory {
    QuickPresets,
    LegacyEqualizerMigration,
}

enum SettingBlock<'a, Message> {
    Row(Element<'a, Message>),
    Standalone(Element<'a, Message>),
    GroupedSection {
        title: Cow<'a, str>,
        rows: Vec<Element<'a, Message>>,
    },
}

impl DeviceSettingsModel {
    pub fn new(
        device: DebugOpenSCQ30Device,
        quick_presets_handler: QuickPresetsHandler,
        config_dir: PathBuf,
    ) -> (Self, Task<Message>) {
        let mut nav_model = nav_bar::Model::default();
        for category in device.categories() {
            nav_model.insert().text(category.translate()).data(category);
        }
        nav_model
            .insert()
            .text(fl!("quick-presets"))
            .data(CustomCategory::QuickPresets);
        nav_model.activate_position(0);

        // watch will close when we drop the device, so this will clean itself up
        let mut watch = device.0.watch_for_changes();
        let stream = cosmic::iced::stream::channel(1, async move |mut output| {
            while watch.changed().await.is_ok() {
                match output.try_send(Message::RefreshSettings) {
                    Err(err) if err.is_disconnected() => return,
                    _ => (),
                }
            }
            debug!("stopping state change watcher task");
        });

        let (quick_presets_model, quick_presets_refresh_task) =
            quick_presets::QuickPresetsModel::new(device.clone(), quick_presets_handler);

        let mut connection_status = device.0.connection_status();
        let watch_for_disconnect_task = Task::future(
            async move {
                loop {
                    if matches!(*connection_status.borrow(), ConnectionStatus::Disconnected) {
                        tracing::info!("device disconnected");
                        return Message::Disconnect;
                    }
                    if connection_status.changed().await.is_err() {
                        // sender is dropped, which means device was dropped, which means DeviceSettingsModel was dropped
                        // in that case, bail
                        tracing::debug!("connection status sender dropped, bailing");
                        return Message::None;
                    }
                }
            }
            .instrument(tracing::info_span!("watch_for_disconnect_task")),
        );

        let mut model = Self {
            throttle: throttle::Throttle::new(device.0.clone()),
            device,
            nav_model,
            settings_order: Vec::new(),
            settings: HashMap::new(),
            dialog: None,
            legacy_equalizer_migration: None,
            quick_presets_model,
        };
        let task = Task::batch([
            model.refresh(),
            quick_presets_refresh_task.map(Into::into),
            Self::initialize_legacy_migration(config_dir),
            Task::stream(stream),
            watch_for_disconnect_task,
        ]);
        (model, task)
    }

    fn initialize_legacy_migration(config_dir: PathBuf) -> Task<Message> {
        Task::future(async move {
            let profiles = match openscq30_v1_migration::all_equalizer_profiles(config_dir).await {
                Ok(profiles) => profiles,
                Err(err) => match err {
                    openscq30_v1_migration::FetchProfilesError::NoLegacyConfig => {
                        return Message::None;
                    }
                    _ => {
                        tracing::error!("error loading legacy config file: {err:?}");
                        return Message::None;
                    }
                },
            };
            Message::AddLegacyEqualizerMigrationPage(profiles)
        })
    }

    pub fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<Message> {
        self.nav_model.activate(id);
        if !self.is_quick_presets_active() {
            self.quick_presets_model.close_drawer();
        }
        self.refresh()
    }

    fn refresh(&mut self) -> Task<Message> {
        Task::batch([self.refresh_settings()])
    }

    fn refresh_settings(&mut self) -> Task<Message> {
        if let Some(category_id) = self.nav_model.active_data::<CategoryId>() {
            let mut settings_order = self.device.settings_in_category(category_id);
            let settings = settings_order
                .iter()
                .copied()
                .flat_map(|setting_id| {
                    self.throttle
                        .setting(&setting_id)
                        .map(|value| (setting_id, value))
                })
                .collect::<HashMap<SettingId, Setting>>();

            // remove setting ids that are currently unavailable from the ordering
            settings_order.retain(|setting_id| settings.contains_key(setting_id));
            self.settings_order = settings_order;

            // remove setting ui state for settings that no longer exist
            self.settings
                .retain(|setting_id, _| settings.contains_key(setting_id));

            // update existing setting ui state, or add new state for newly introduced settings
            for (setting_id, setting) in settings.into_iter() {
                match self.settings.entry(setting_id) {
                    hash_map::Entry::Occupied(mut occupied_entry) => {
                        occupied_entry.get_mut().variant_state.update(setting);
                    }
                    hash_map::Entry::Vacant(vacant_entry) => {
                        vacant_entry.insert(SettingUiState {
                            setting_id,
                            translated_name: setting_id.translate(),
                            variant_state: setting.into(),
                        });
                    }
                }
            }
        }
        Task::none()
    }

    /// Returns `None` because category navigation is rendered responsively inside `view()`
    /// so categories stay directly visible and clickable even below libcosmic's 648px collapse width.
    pub fn nav_model(&self) -> Option<&nav_bar::Model> {
        None
    }

    fn is_quick_presets_active(&self) -> bool {
        matches!(
            self.nav_model.active_data(),
            Some(CustomCategory::QuickPresets)
        )
    }

    pub fn is_drawer_open(&self) -> bool {
        self.is_quick_presets_active() && self.quick_presets_model.is_drawer_open()
    }

    pub fn has_dialog(&self) -> bool {
        self.dialog.is_some() || self.quick_presets_model.has_dialog()
    }

    pub fn dismiss_dialog(&mut self) -> bool {
        if self.quick_presets_model.close_dialog() {
            return true;
        }
        if self.dialog.is_some() {
            self.dialog = None;
            return true;
        }
        false
    }

    pub fn dismiss_drawer(&mut self) -> bool {
        if self.is_quick_presets_active() {
            self.quick_presets_model.close_drawer()
        } else {
            false
        }
    }

    #[cfg(test)]
    pub fn open_quick_preset_drawer_for_test(&mut self) {
        let quick_presets_id = self
            .nav_model
            .iter()
            .find(|&id| {
                matches!(
                    self.nav_model.data::<CustomCategory>(id),
                    Some(CustomCategory::QuickPresets)
                )
            })
            .expect("QuickPresets category should be present");
        let _ = self.on_nav_select(quick_presets_id);
        let _ = self.update(Message::QuickPresets(
            quick_presets::Message::SetQuickPresets(vec![openscq30_lib::storage::QuickPreset {
                name: "Test".to_string(),
                fields: Vec::new(),
            }]),
        ));
        let _ = self.update(Message::QuickPresets(
            quick_presets::Message::EditQuickPreset(0),
        ));
    }

    fn modifiable_select_add_input_id() -> widget::Id {
        widget::Id::new(MODIFIABLE_SELECT_ADD_INPUT_ID)
    }

    fn modifiable_select_add_text_input(name: &str) -> widget::text_input::TextInput<'_, Message> {
        widget::text_input(fl!("name"), name)
            .id(Self::modifiable_select_add_input_id())
            .style(theme::apple_text_input())
            .on_input(Message::ModifiableSelectAddDialogSetName)
            .on_submit(|name| Message::ModifiableSelectAddDialogSubmit(Some(name)))
    }

    pub fn dialog(&self) -> Option<Element<'_, Message>> {
        self.quick_presets_model
            .dialog()
            .map(|dialog| dialog.map(Into::into))
            .or_else(|| {
                self.dialog.as_ref().map(|dialog| match dialog {
                    Dialog::ModifiableSelectAdd(setting_id, name) => widget::dialog()
                        .title({
                            let setting_name = setting_id.translate();
                            fl!("add-item", name = setting_name.as_str())
                        })
                        .control(Self::modifiable_select_add_text_input(name))
                        .primary_action(
                            widget::button::suggested(fl!("create"))
                                .on_press(Message::ModifiableSelectAddDialogSubmit(None)),
                        )
                        .secondary_action(
                            widget::button::destructive(fl!("cancel"))
                                .on_press(Message::CancelDialog),
                        )
                        .into(),
                    Dialog::ModifiableSelectRemove(_setting_id, name) => widget::dialog()
                        .title(fl!("remove-item", name = name.as_ref()))
                        .body(fl!("remove-item-confirm", name = name.as_ref()))
                        .primary_action(
                            widget::button::destructive(fl!("remove"))
                                .on_press(Message::ModifiableSelectRemoveDialogSubmit),
                        )
                        .secondary_action(
                            widget::button::text(fl!("cancel")).on_press(Message::CancelDialog),
                        )
                        .into(),
                    Dialog::ImportStringConfirm(setting_id, _text) => widget::dialog()
                        .title(setting_id.translate())
                        .body(
                            if let Some(SettingVariantUiState::ImportString {
                                confirmation_message: Some(confirmation_message),
                                text: _,
                            }) = self
                                .settings
                                .get(setting_id)
                                .map(|setting| &setting.variant_state)
                            {
                                confirmation_message.as_str()
                            } else {
                                ""
                            },
                        )
                        .primary_action(
                            widget::button::suggested(fl!("confirm"))
                                .on_press(Message::ConfirmImportString),
                        )
                        .secondary_action(
                            widget::button::text(fl!("cancel")).on_press(Message::CancelDialog),
                        )
                        .into(),
                })
            })
    }

    pub fn view(&self) -> Element<'_, Message> {
        widget::responsive(|size| {
            let show_sidebar = size.width >= SIDEBAR_BREAKPOINT;
            let content_width = if show_sidebar {
                (size.width - 236.0).max(200.0)
            } else {
                size.width
            };
            let is_narrow = content_width < NARROW_BREAKPOINT;

            let active_view: Element<'_, Message> =
                if let Some(custom_category) = self.nav_model.active_data::<CustomCategory>() {
                    match custom_category {
                        CustomCategory::QuickPresets => {
                            self.quick_presets_model.view(is_narrow).map(Into::into)
                        }
                        CustomCategory::LegacyEqualizerMigration => {
                            if let Some(model) = &self.legacy_equalizer_migration {
                                model.view(is_narrow).map(Message::LegacyMigration)
                            } else {
                                widget::space().into()
                            }
                        }
                    }
                } else if let Some(category_id) = self.nav_model.active_data::<CategoryId>() {
                    if *category_id == CategoryId::Equalizer {
                        self.view_equalizer_workspace(category_id, content_width, is_narrow)
                    } else {
                        self.view_settings(category_id, is_narrow)
                    }
                } else {
                    widget::space().into()
                };

            let transition_key = self
                .nav_model
                .position(self.nav_model.active())
                .unwrap_or(0) as u64;
            let active_view = fluid::fluid_transition(transition_key, active_view);

            if show_sidebar {
                let sidebar = self.view_sidebar();
                widget::row![
                    widget::container(sidebar)
                        .padding([SPACE_LG, 0, SPACE_LG, SPACE_LG])
                        .height(Length::Fill),
                    widget::container(active_view)
                        .width(Length::Fill)
                        .height(Length::Fill),
                ]
                .spacing(SPACE_SM)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            } else {
                let pill_bar = self.view_compact_category_pills();
                widget::column![
                    pill_bar,
                    cosmic::iced::widget::rule::horizontal(1)
                        .class(cosmic::theme::Rule::custom(theme::hairline_rule)),
                    widget::container(active_view)
                        .width(Length::Fill)
                        .height(Length::Fill),
                ]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
        })
        .into()
    }

    fn category_badge_visual(&self, entity: nav_bar::Id) -> (cosmic::iced::Color, &'static str) {
        use cosmic::iced::Color;
        if let Some(cat) = self.nav_model.data::<CategoryId>(entity) {
            match cat {
                CategoryId::SoundModes => {
                    (Color::from_rgb8(255, 45, 85), "audio-speakers-symbolic")
                }
                CategoryId::Equalizer | CategoryId::EqualizerImportExport => (
                    Color::from_rgb8(0, 122, 255),
                    "multimedia-equalizer-symbolic",
                ),
                CategoryId::HearingAssistance => (
                    Color::from_rgb8(50, 173, 230),
                    "preferences-desktop-accessibility-symbolic",
                ),
                CategoryId::ButtonConfiguration => {
                    (Color::from_rgb8(142, 142, 147), "input-touchpad-symbolic")
                }
                CategoryId::General => {
                    (Color::from_rgb8(88, 86, 214), "preferences-system-symbolic")
                }
                CategoryId::DeviceInformation => {
                    (Color::from_rgb8(52, 199, 89), "dialog-information-symbolic")
                }
                _ => (
                    Color::from_rgb8(142, 142, 147),
                    "preferences-other-symbolic",
                ),
            }
        } else {
            (Color::from_rgb8(255, 149, 0), "starred-symbolic")
        }
    }

    fn view_sidebar(&self) -> Element<'_, Message> {
        let active_entity = self.nav_model.active();
        let device_header = widget::row![
            widget::icon(crate::icons::openscq30()).size(28),
            widget::column![
                theme::row_title(self.device.model().translate()).wrapping(Wrapping::Word),
                theme::muted_caption(self.device.model().to_string()),
            ]
            .spacing(2)
            .width(Length::Fill),
        ]
        .spacing(SPACE_SM)
        .align_y(alignment::Vertical::Center)
        .padding([SPACE_XS, SPACE_SM]);

        let nav_buttons = widget::column(self.nav_model.iter().map(|entity| {
            let label = self.nav_model.text(entity).unwrap_or_default().to_string();
            let is_active = entity == active_entity;
            let (sq_color, icon_name) = self.category_badge_visual(entity);

            let squircle = widget::container(widget::icon::from_name(icon_name).size(14))
                .width(Length::Fixed(26.0))
                .height(Length::Fixed(26.0))
                .align_x(alignment::Horizontal::Center)
                .align_y(alignment::Vertical::Center)
                .class(cosmic::theme::Container::custom(
                    theme::category_squircle_container(sq_color),
                ));

            fluid::fluid_pressable(
                widget::button::custom(
                    widget::row![
                        squircle,
                        widget::text::body(label.clone())
                            .wrapping(Wrapping::Word)
                            .width(Length::Fill),
                        theme::muted_caption("›"),
                    ]
                    .spacing(SPACE_SM)
                    .align_y(alignment::Vertical::Center)
                    .width(Length::Fill),
                )
                .name(label)
                .class(theme::nav_pill_button(is_active))
                .padding([SPACE_SM, SPACE_SM])
                .width(Length::Fill)
                .on_press(Message::SelectCategory(entity)),
            )
        }))
        .spacing(4)
        .width(Length::Fill);

        widget::column![
            device_header,
            cosmic::iced::widget::rule::horizontal(1)
                .class(cosmic::theme::Rule::custom(theme::hairline_rule)),
            widget::scrollable(nav_buttons).height(Length::Fill),
        ]
        .spacing(SPACE_SM)
        .padding(SPACE_MD)
        .width(244)
        .height(Length::Fill)
        .apply(widget::container)
        .class(cosmic::theme::Container::custom(theme::card_container))
        .height(Length::Fill)
        .into()
    }

    fn view_compact_category_pills(&self) -> Element<'_, Message> {
        let active_entity = self.nav_model.active();
        let pills: Vec<Element<'_, Message>> = self
            .nav_model
            .iter()
            .map(|entity| {
                let label = self.nav_model.text(entity).unwrap_or_default().to_string();
                let is_active = entity == active_entity;
                fluid::fluid_pressable(
                    widget::button::custom(widget::text::caption_heading(label.clone()))
                        .name(label)
                        .class(theme::card_button(is_active))
                        .padding([6, 12])
                        .on_press(Message::SelectCategory(entity)),
                )
            })
            .collect();

        widget::flex_row(pills)
            .column_spacing(6)
            .row_spacing(6)
            .width(Length::Fill)
            .apply(widget::container)
            .padding([SPACE_SM, SPACE_LG])
            .width(Length::Fill)
            .into()
    }

    fn view_settings<'a>(
        &'a self,
        category_id: &'a CategoryId,
        is_narrow: bool,
    ) -> Element<'a, Message> {
        let is_first_category = self
            .nav_model
            .iter()
            .next()
            .is_some_and(|first| first == self.nav_model.active());

        let mut sections: Vec<Element<'a, Message>> = Vec::new();
        if is_first_category
            || matches!(
                category_id,
                CategoryId::SoundModes | CategoryId::General | CategoryId::DeviceInformation
            )
        {
            sections.push(ios_hero::view_device_hero(&self.device));
        }
        if !is_first_category {
            sections.push(components::page_header(category_id.translate(), None, None));
        }

        let mut pending_rows: Vec<Element<'a, Message>> = Vec::new();

        let flush_pending = |pending: &mut Vec<Element<'a, Message>>,
                             sections: &mut Vec<Element<'a, Message>>| {
            if !pending.is_empty() {
                let rows = std::mem::take(pending);
                sections.push(components::inset_grouped_section(None, rows));
            }
        };

        for setting_id in &self.settings_order {
            let Some(setting_ui_state) = self.settings.get(setting_id) else {
                tracing::error!("setting id {setting_id} is missing ui state");
                continue;
            };
            match self.view_setting(setting_ui_state, is_narrow) {
                SettingBlock::Row(element) => pending_rows.push(element),
                SettingBlock::Standalone(element) => {
                    flush_pending(&mut pending_rows, &mut sections);
                    sections.push(element);
                }
                SettingBlock::GroupedSection { title, rows } => {
                    if !rows.is_empty() {
                        flush_pending(&mut pending_rows, &mut sections);
                        sections.push(components::inset_grouped_section(Some(title), rows));
                    }
                }
            }
        }
        flush_pending(&mut pending_rows, &mut sections);

        widget::scrollable(components::page_frame(
            widget::column(sections)
                .spacing(SPACE_LG)
                .width(Length::Fill),
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_equalizer_workspace<'a>(
        &'a self,
        category_id: &'a CategoryId,
        available_width: f32,
        is_narrow: bool,
    ) -> Element<'a, Message> {
        let eq_state = self
            .settings_order
            .iter()
            .filter_map(|id| self.settings.get(id))
            .find(|s| matches!(s.variant_state, SettingVariantUiState::Equalizer { .. }));

        let Some(eq_state) = eq_state else {
            return self.view_settings(category_id, is_narrow);
        };

        let SettingVariantUiState::Equalizer {
            setting: eq_setting,
            read_only,
            value: eq_values,
            line: eq_line,
        } = &eq_state.variant_state
        else {
            unreachable!();
        };

        let preset_state = self
            .settings_order
            .iter()
            .filter_map(|id| self.settings.get(id))
            .find(|s| {
                matches!(
                    s.variant_state,
                    SettingVariantUiState::PresetEqualizerProfileSelect { .. }
                )
            });

        let custom_profile_state = self
            .settings_order
            .iter()
            .filter_map(|id| self.settings.get(id))
            .find(|s| {
                matches!(
                    s.variant_state,
                    SettingVariantUiState::ModifiableSelect { .. }
                )
            });

        let active_preset_label = preset_state.and_then(|ps| {
            if let SettingVariantUiState::PresetEqualizerProfileSelect {
                previews, value, ..
            } = &ps.variant_state
            {
                let v = value.as_ref()?;
                previews
                    .iter()
                    .find(|(p, _)| p.option.as_ref() == v.as_ref())
                    .map(|(p, _)| p.localized_name.clone())
            } else {
                None
            }
        });

        let range_label = format!(
            "{} to {} dB",
            format_db_value(eq_setting.min, eq_setting.fraction_digits),
            format_db_value(eq_setting.max, eq_setting.fraction_digits),
        );

        // 1. Hero Live Curve Card
        let hz_ticks: Vec<Element<'_, Message>> = if is_narrow && eq_setting.band_hz.len() > 4 {
            let len = eq_setting.band_hz.len();
            [0, len / 2, len - 1]
                .into_iter()
                .map(|idx| {
                    theme::muted_caption(format_hz_label(eq_setting.band_hz[idx]))
                        .width(Length::Fill)
                        .align_x(if idx == 0 {
                            alignment::Horizontal::Left
                        } else if idx == len - 1 {
                            alignment::Horizontal::Right
                        } else {
                            alignment::Horizontal::Center
                        })
                        .into()
                })
                .collect()
        } else {
            eq_setting
                .band_hz
                .iter()
                .map(|&hz| {
                    theme::muted_caption(format_hz_label(hz))
                        .width(Length::Fill)
                        .align_x(alignment::Horizontal::Center)
                        .into()
                })
                .collect()
        };

        let title_widget = theme::row_title(category_id.translate()).wrapping(Wrapping::Word);
        let range_widget = theme::mono_value(range_label).wrapping(Wrapping::Word);
        let preset_badge = active_preset_label
            .as_ref()
            .map(|label| components::pill_badge(label.clone(), SemanticTone::Accent));

        let hero_header: Element<'_, Message> = if is_narrow {
            let mut meta_items: Vec<Element<'_, Message>> = vec![range_widget.into()];
            if let Some(badge) = preset_badge {
                meta_items.push(badge);
            }
            let meta_row = widget::flex_row(meta_items)
                .column_spacing(SPACE_SM)
                .row_spacing(SPACE_XS)
                .align_items(alignment::Alignment::Center)
                .width(Length::Fill);

            widget::column![title_widget, meta_row]
                .spacing(SPACE_XS)
                .width(Length::Fill)
                .into()
        } else {
            let mut header_items: Vec<Element<'_, Message>> =
                vec![title_widget.into(), range_widget.into()];
            if let Some(badge) = preset_badge {
                header_items.push(badge);
            }
            widget::flex_row(header_items)
                .column_spacing(SPACE_SM)
                .row_spacing(SPACE_XS)
                .align_items(alignment::Alignment::Center)
                .width(Length::Fill)
                .into()
        };

        let hero_card = components::elevated_card(
            widget::column![
                hero_header,
                widget::container(eq_line.view())
                    .width(Length::Fill)
                    .height(104),
                widget::row(hz_ticks).width(Length::Fill),
            ]
            .spacing(SPACE_SM)
            .width(Length::Fill),
        );

        // 2. Custom Profile Bar (ModifiableSelect)
        let custom_profile_section = custom_profile_state.and_then(|state| {
            if let SettingVariantUiState::ModifiableSelect { setting, value } = &state.variant_state
            {
                let sid = state.setting_id;
                let row = controls::modifiable_select(
                    Cow::Borrowed(state.translated_name.as_str()),
                    setting,
                    value.as_deref(),
                    move |val| Message::SetSetting(sid, Cow::from(val.to_owned()).into()),
                    Message::ShowModifiableSelectAddDialog(sid),
                    Message::ShowModifiableSelectRemoveDialog(sid),
                    is_narrow,
                );
                Some(components::inset_grouped_section(None, vec![row]))
            } else {
                None
            }
        });

        // 3. 8-Band Horizontal Sliders
        let bands_section = (!read_only).then(|| {
            let sid = eq_state.setting_id;
            let band_rows =
                controls::horizontal_equalizer(eq_setting, eq_values, move |idx, val| {
                    Message::SetEqualizerBand(sid, idx, val)
                });
            components::inset_grouped_section(Some(fl!("bands").into()), band_rows)
        });

        // 4. Preset Curve Preview Gallery (clickable cards, NO conflicting dropdown popup)
        let preset_gallery: Option<Element<'_, Message>> = preset_state.and_then(|ps| {
            if let SettingVariantUiState::PresetEqualizerProfileSelect {
                previews, value, ..
            } = &ps.variant_state
            {
                let sid = ps.setting_id;
                let none_selected = value.is_none();
                let card_width = if is_narrow { 164.0 } else { 148.0 };

                let header = widget::row![
                    theme::section_title(ps.translated_name.as_str()).width(Length::Fill),
                    widget::button::custom(widget::text::caption_heading(fl!("none")))
                        .name(fl!("none"))
                        .class(theme::card_button(none_selected))
                        .padding([4, 12])
                        .on_press(Message::SetSetting(
                            sid,
                            Option::<Cow<'static, str>>::None.into(),
                        )),
                ]
                .spacing(SPACE_SM)
                .align_y(alignment::Vertical::Center);

                let cards: Vec<Element<'_, Message>> = previews
                    .iter()
                    .map(|(preview, line)| {
                        widget::button::custom(
                            widget::column![
                                widget::container(line.view())
                                    .width(Length::Fill)
                                    .height(28),
                                widget::text::caption_heading(&preview.localized_name)
                                    .wrapping(Wrapping::Word),
                            ]
                            .spacing(SPACE_XS)
                            .width(Length::Fill)
                            .padding([SPACE_SM, SPACE_MD]),
                        )
                        .name(&preview.localized_name)
                        .class(theme::card_button(preview.is_active))
                        .width(card_width)
                        .on_press(Message::SetSetting(
                            sid,
                            Some(preview.option.clone()).into(),
                        ))
                        .into()
                    })
                    .collect();

                let gallery = widget::flex_row(cards)
                    .column_spacing(SPACE_SM)
                    .row_spacing(SPACE_SM)
                    .width(Length::Fill);

                Some(
                    widget::column![header, gallery]
                        .spacing(SPACE_SM)
                        .width(Length::Fill)
                        .into(),
                )
            } else {
                None
            }
        });

        // 5. Any remaining settings in Equalizer category
        let extra_rows: Vec<Element<'_, Message>> = self
            .settings_order
            .iter()
            .filter_map(|id| self.settings.get(id))
            .filter(|s| {
                !matches!(
                    s.variant_state,
                    SettingVariantUiState::Equalizer { .. }
                        | SettingVariantUiState::PresetEqualizerProfileSelect { .. }
                        | SettingVariantUiState::ModifiableSelect { .. }
                )
            })
            .flat_map(|s| match self.view_setting(s, is_narrow) {
                SettingBlock::Row(el) | SettingBlock::Standalone(el) => vec![el],
                SettingBlock::GroupedSection { rows, .. } => rows,
            })
            .collect();

        let left_column = widget::column![hero_card]
            .push_maybe(custom_profile_section)
            .push_maybe(bands_section)
            .spacing(SPACE_MD)
            .width(Length::Fill);

        let body: Element<'_, Message> = if available_width >= EQ_TWO_COLUMN_BREAKPOINT
            && let Some(gallery) = preset_gallery
        {
            widget::column![
                widget::row![
                    left_column.width(Length::FillPortion(5)),
                    widget::container(gallery).width(Length::FillPortion(6)),
                ]
                .spacing(SPACE_LG)
                .width(Length::Fill),
            ]
            .push_maybe(
                (!extra_rows.is_empty())
                    .then(|| components::inset_grouped_section(None, extra_rows)),
            )
            .spacing(SPACE_LG)
            .width(Length::Fill)
            .into()
        } else {
            widget::column![left_column]
                .push_maybe(preset_gallery)
                .push_maybe(
                    (!extra_rows.is_empty())
                        .then(|| components::inset_grouped_section(None, extra_rows)),
                )
                .spacing(SPACE_LG)
                .width(Length::Fill)
                .into()
        };

        let page = widget::container(
            widget::container(body)
                .width(Length::Fill)
                .max_width(1160.0),
        )
        .width(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .padding([SPACE_LG, SPACE_LG]);

        widget::scrollable(page)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_setting<'a>(
        &'a self,
        setting: &'a SettingUiState,
        is_narrow: bool,
    ) -> SettingBlock<'a, Message> {
        let setting_id = setting.setting_id;
        let translated_name = Cow::Borrowed(setting.translated_name.as_str());
        match &setting.variant_state {
            SettingVariantUiState::Toggle { value } => SettingBlock::Row(controls::toggle(
                translated_name,
                *value,
                move |new_value| Message::SetSetting(setting_id, new_value.into()),
            )),
            SettingVariantUiState::I32Range { setting, value } => {
                SettingBlock::Row(controls::i32_range(
                    translated_name,
                    setting.clone(),
                    *value,
                    move |new_value| Message::SetSetting(setting_id, new_value.into()),
                    is_narrow,
                ))
            }
            SettingVariantUiState::Select { setting, value } => {
                if matches!(
                    setting_id,
                    SettingId::ListeningMode | SettingId::AmbientSoundMode
                ) {
                    SettingBlock::Standalone(ios_hero::ios_segmented_control(
                        Some(translated_name),
                        setting,
                        value,
                        true,
                        move |value| {
                            Message::SetSetting(setting_id, Cow::from(value.to_owned()).into())
                        },
                    ))
                } else {
                    SettingBlock::Row(controls::select(
                        translated_name,
                        setting,
                        value,
                        move |value| {
                            Message::SetSetting(setting_id, Cow::from(value.to_owned()).into())
                        },
                        is_narrow,
                    ))
                }
            }
            SettingVariantUiState::OptionalSelect { setting, value }
            | SettingVariantUiState::PresetEqualizerProfileSelect {
                select: setting,
                value,
                ..
            } => SettingBlock::Row(controls::optional_select(
                translated_name,
                setting,
                value.as_deref(),
                move |value| {
                    Message::SetSetting(
                        setting_id,
                        value.map(ToOwned::to_owned).map(Cow::from).into(),
                    )
                },
                is_narrow,
            )),
            SettingVariantUiState::ModifiableSelect { setting, value } => {
                SettingBlock::Row(controls::modifiable_select(
                    translated_name,
                    setting,
                    value.as_deref(),
                    move |value| {
                        Message::SetSetting(setting_id, Cow::from(value.to_owned()).into())
                    },
                    Message::ShowModifiableSelectAddDialog(setting_id),
                    Message::ShowModifiableSelectRemoveDialog(setting_id),
                    is_narrow,
                ))
            }
            SettingVariantUiState::MultiSelect { setting, values } => {
                SettingBlock::GroupedSection {
                    title: translated_name.clone(),
                    rows: controls::multi_select(translated_name, setting, values, move |values| {
                        Message::SetSetting(setting_id, values.into())
                    }),
                }
            }
            SettingVariantUiState::MultiSelectWithRemove { setting, values } => {
                SettingBlock::GroupedSection {
                    title: translated_name.clone(),
                    rows: controls::multi_select_with_remove(
                        translated_name,
                        setting,
                        values,
                        move |values| Message::SetSetting(setting_id, values.into()),
                        |option| {
                            Message::SetSetting(
                                setting_id,
                                Value::MultiSelectWithRemoveCommand(
                                    settings::MultiSelectWithRemoveCommand::Remove(option),
                                ),
                            )
                        },
                    ),
                }
            }
            SettingVariantUiState::Equalizer {
                setting,
                read_only,
                value,
                line,
            } => {
                if !read_only {
                    SettingBlock::GroupedSection {
                        title: translated_name,
                        rows: controls::horizontal_equalizer(
                            setting,
                            value,
                            move |index, value| Message::SetEqualizerBand(setting_id, index, value),
                        ),
                    }
                } else {
                    SettingBlock::Row(
                        widget::container(line.view())
                            .width(Length::Fill)
                            .height(104)
                            .into(),
                    )
                }
            }
            SettingVariantUiState::Information { translated_value } => {
                SettingBlock::Row(controls::information(
                    translated_name,
                    Cow::Borrowed(translated_value),
                    Message::CopyToClipboard(translated_value.to_owned()),
                    is_narrow,
                ))
            }
            SettingVariantUiState::ImportString {
                text,
                confirmation_message: _,
            } => SettingBlock::Row(controls::import_string(
                translated_name,
                Cow::Borrowed(text),
                move |text| Message::SetImportString(setting_id, text),
                move |text| Message::AskConfirmImportString(setting_id, Cow::from(text).into()),
                is_narrow,
            )),
            SettingVariantUiState::HueColorPicker { hue } => {
                SettingBlock::Row(controls::hue_color_picker(
                    translated_name,
                    *hue,
                    move |new_hue| Message::SetSetting(setting_id, new_hue.into()),
                    is_narrow,
                ))
            }
            SettingVariantUiState::Action => SettingBlock::Row(controls::action(
                translated_name,
                Message::SetSetting(setting_id, true.into()),
                is_narrow,
            )),
            SettingVariantUiState::TimeOfDay {
                minutes_after_midnight,
                hour_text,
                minute_text,
            } => SettingBlock::Row(controls::time_of_day(
                translated_name,
                controls::TimeOfDayValues {
                    maybe_is_pm: Some(minutes_after_midnight.is_pm()),
                    hour_text: hour_text.as_ref().map_or_else(
                        || Cow::Owned(minutes_after_midnight.twelve_hour().to_string()),
                        |text| Cow::Borrowed(text.as_str()),
                    ),
                    minute_text: minute_text.as_ref().map_or_else(
                        || Cow::Owned(format!("{:02}", minutes_after_midnight.minute())),
                        |text| Cow::Borrowed(text.as_str()),
                    ),
                },
                move |hour| Message::SetTimeTwelveHour(setting_id, hour),
                move |minute| Message::SetTimeMinute(setting_id, minute),
                move |is_pm| Message::SetTimePm(setting_id, is_pm),
                Message::RefreshTimeTwelveHour(setting_id),
                is_narrow,
            )),
        }
    }

    pub fn context_drawer(&self) -> Option<ContextDrawer<'_, Message>> {
        if self.is_quick_presets_active() {
            self.quick_presets_model
                .context_drawer()
                .map(|context_drawer| context_drawer.map(Into::into))
        } else {
            None
        }
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::SelectCategory(id) => Action::Task(self.on_nav_select(id)),
            Message::QuickPresets(inner) => match self.quick_presets_model.update(inner) {
                quick_presets::Action::Warning(text) => Action::Warning(text),
                quick_presets::Action::None => Action::None,
                quick_presets::Action::Task(task) => Action::Task(task.map(Into::into)),
                quick_presets::Action::FocusTextInput(id) => Action::FocusTextInput(id),
            },
            Message::SetSetting(setting_id, value) => self.set_setting(setting_id, value),
            Message::SetTimeTwelveHour(setting_id, new_hour_text) => {
                let maybe_value = if new_hour_text.len() <= 2
                    && let Some(setting) = self.settings.get_mut(&setting_id)
                    && let SettingVariantUiState::TimeOfDay {
                        minutes_after_midnight,
                        hour_text,
                        minute_text,
                    } = &mut setting.variant_state
                {
                    *minute_text = None;
                    if let Ok(hour) = new_hour_text.parse() {
                        *minutes_after_midnight = minutes_after_midnight.with_twelve_hour(hour);
                        // If hour is out of range and gets clamped, update the text to match
                        *hour_text =
                            (hour == minutes_after_midnight.twelve_hour()).then_some(new_hour_text);
                        Some(minutes_after_midnight.0.into())
                    } else if new_hour_text.is_empty() {
                        *hour_text = Some(new_hour_text);
                        None
                    } else {
                        *hour_text = None;
                        None
                    }
                } else {
                    None
                };
                if let Some(value) = maybe_value {
                    self.set_setting(setting_id, value)
                } else {
                    Action::None
                }
            }
            Message::SetTimeMinute(setting_id, new_minute_text) => {
                let maybe_value = if new_minute_text.len() <= 2
                    && let Some(setting) = self.settings.get_mut(&setting_id)
                    && let SettingVariantUiState::TimeOfDay {
                        minutes_after_midnight,
                        hour_text,
                        minute_text,
                    } = &mut setting.variant_state
                {
                    *hour_text = None;
                    if let Ok(minute) = new_minute_text.parse() {
                        *minutes_after_midnight = minutes_after_midnight.with_minute(minute);
                        // If minute is out of range and gets clamped, update the text to match
                        *minute_text =
                            (minute == minutes_after_midnight.minute()).then_some(new_minute_text);
                        Some(minutes_after_midnight.0.into())
                    } else if new_minute_text.is_empty() {
                        *minute_text = Some(new_minute_text);
                        None
                    } else {
                        *minute_text = None;
                        None
                    }
                } else {
                    None
                };
                // outside of the if statement to avoid double mutable borrow
                if let Some(value) = maybe_value {
                    self.set_setting(setting_id, value)
                } else {
                    Action::None
                }
            }
            Message::SetTimePm(setting_id, is_pm) => {
                let maybe_value = if let Some(setting) = self.settings.get_mut(&setting_id)
                    && let SettingVariantUiState::TimeOfDay {
                        minutes_after_midnight,
                        hour_text,
                        minute_text,
                    } = &mut setting.variant_state
                {
                    *minutes_after_midnight = minutes_after_midnight.with_pm(is_pm);
                    *hour_text = None;
                    *minute_text = None;
                    Some(minutes_after_midnight.0.into())
                } else {
                    None
                };

                if let Some(value) = maybe_value {
                    self.set_setting(setting_id, value)
                } else {
                    Action::None
                }
            }
            Message::RefreshTimeTwelveHour(setting_id) => {
                if let Some(setting) = self.settings.get_mut(&setting_id)
                    && let SettingVariantUiState::TimeOfDay {
                        hour_text,
                        minute_text,
                        ..
                    } = &mut setting.variant_state
                {
                    *hour_text = None;
                    *minute_text = None;
                }
                Action::None
            }
            Message::SetEqualizerBand(setting_id, index, new_value) => {
                if let Some(Setting::Equalizer {
                    setting: _,
                    read_only: _,
                    value: values,
                }) = self.throttle.setting(&setting_id)
                {
                    let mut new_values = values.clone();
                    if let Some(slot) = new_values.get_mut(index as usize) {
                        *slot = new_value;
                    }
                    let maybe_task = self.throttle.set_setting(setting_id, new_values.into());
                    _ = self.refresh_settings();
                    maybe_task.map_or(Action::None, |task| Action::Task(task.map(Into::into)))
                } else {
                    Action::None
                }
            }
            Message::RefreshSettings => Action::Task(self.refresh_settings()),
            Message::Warning(message) => Action::Warning(message),
            Message::CancelDialog => {
                self.dialog = None;
                Action::None
            }
            Message::ShowModifiableSelectAddDialog(setting_id) => {
                self.dialog = Some(Dialog::ModifiableSelectAdd(setting_id, String::new()));
                Action::FocusTextInput(Self::modifiable_select_add_input_id())
            }
            Message::ShowModifiableSelectRemoveDialog(setting_id) => {
                let selected_item = self.settings.get(&setting_id).and_then(|item| {
                    if let SettingVariantUiState::ModifiableSelect { setting: _, value } =
                        &item.variant_state
                    {
                        value.to_owned()
                    } else {
                        None
                    }
                });
                if let Some(selected_item) = selected_item {
                    self.dialog = Some(Dialog::ModifiableSelectRemove(setting_id, selected_item));
                } else {
                    tracing::error!(
                        r#"tried to open modifiable select remove dialog for {setting_id:?}, but selected item is None.
                        current settings: {:?}
                        "#,
                        self.settings,
                    );
                }
                Action::None
            }
            Message::ModifiableSelectAddDialogSetName(new_name) => {
                if let Some(Dialog::ModifiableSelectAdd(_setting_id, name)) = &mut self.dialog {
                    *name = new_name;
                }
                Action::None
            }
            Message::ModifiableSelectAddDialogSubmit(override_name) => {
                if let Some(Dialog::ModifiableSelectAdd(setting_id, name)) = self.dialog.take() {
                    let name = override_name.unwrap_or(name);

                    let device = self.device.clone();
                    Action::Task(
                        Task::future(async move {
                            device
                                .set_setting_values(vec![(
                                    setting_id,
                                    Value::ModifiableSelectCommand(
                                        settings::ModifiableSelectCommand::Add(name.into()),
                                    ),
                                )])
                                .await
                                .map_err(handle_soft_error!())?;
                            Ok(Message::RefreshSettings)
                        })
                        .map(coalesce_result),
                    )
                } else {
                    Action::None
                }
            }
            Message::ModifiableSelectRemoveDialogSubmit => {
                if let Some(Dialog::ModifiableSelectRemove(setting_id, name)) = self.dialog.take() {
                    let device = self.device.clone();
                    Action::Task(
                        Task::future(async move {
                            device
                                .set_setting_values(vec![(
                                    setting_id,
                                    Value::ModifiableSelectCommand(
                                        settings::ModifiableSelectCommand::Remove(name),
                                    ),
                                )])
                                .await
                                .map_err(handle_soft_error!())?;
                            Ok(Message::RefreshSettings)
                        })
                        .map(coalesce_result),
                    )
                } else {
                    Action::None
                }
            }
            Message::AddLegacyEqualizerMigrationPage(profiles) => {
                self.legacy_equalizer_migration = Some(LegacyMigrationModel::new(profiles));
                self.nav_model
                    .insert()
                    .text(fl!("legacy-equalizer-profile-migration"))
                    .data(CustomCategory::LegacyEqualizerMigration);

                Action::None
            }
            Message::None => Action::None,
            Message::LegacyMigration(message) => match message {
                legacy_migration::Message::Migrate(name, volume_adjustments) => {
                    let device = self.device.to_owned();
                    Action::Task(
                        Task::<anyhow::Result<Message>>::future(async move {
                            openscq30_v1_migration::migrate_legacy_profile(
                                device.as_ref(),
                                name,
                                volume_adjustments,
                            )
                            .await?;
                            Ok(Message::RefreshSettings)
                        })
                        .map(|r| r.map_err(handle_soft_error!()))
                        .map(coalesce_result),
                    )
                }
            },
            Message::CopyToClipboard(text) => Action::Task(cosmic::iced::clipboard::write(text)),
            Message::SetImportString(setting_id, new_text) => {
                if let Some(setting) = self.settings.get_mut(&setting_id)
                    && let SettingVariantUiState::ImportString { text, .. } =
                        &mut setting.variant_state
                {
                    *text = new_text;
                }
                Action::None
            }
            Message::AskConfirmImportString(setting_id, import_text) => {
                self.dialog = Some(Dialog::ImportStringConfirm(setting_id, import_text));
                Action::None
            }
            Message::ConfirmImportString => {
                if let Some(Dialog::ImportStringConfirm(setting_id, text)) = self.dialog.take() {
                    if let Some(setting) = self.settings.get_mut(&setting_id)
                        && let SettingVariantUiState::ImportString { text, .. } =
                            &mut setting.variant_state
                    {
                        *text = String::new();
                    }
                    let device = self.device.clone();
                    Action::Task(
                        Task::future(async move {
                            device
                                .set_setting_values(vec![(setting_id, Cow::from(text).into())])
                                .await
                                .map_err(handle_soft_error!())?;
                            Ok(Message::RefreshSettings)
                        })
                        .map(coalesce_result),
                    )
                } else {
                    Action::None
                }
            }
            Message::Throttle(message) => match self.throttle.update(message) {
                throttle::Action::Task(task) => Action::Task(task.map(Into::into)),
                throttle::Action::Error(err) => Action::Task(Task::done(handle_soft_error!()(err))),
                throttle::Action::None => Action::None,
            },
            Message::Disconnect => Action::Disconnect,
        }
    }

    fn set_setting(&mut self, setting_id: SettingId, value: Value) -> Action {
        let device = self.device.clone();
        let should_throttle = matches!(
            device.setting(&setting_id),
            Some(
                // throttled because it's implemented as a slider, which would update every tick
                Setting::I32Range { .. }
                    // throttled because it's implemented as a slider, which would update every tick
                    | Setting::HueColorPicker { .. }
                    // throttled so that if the user types both digits of an hour or minute
                    // quickly, we only update once
                    | Setting::TimeOfDay { .. }
            ),
        );
        if should_throttle {
            let maybe_task = self.throttle.set_setting(setting_id, value);
            _ = self.refresh_settings();
            maybe_task.map_or(Action::None, |task| Action::Task(task.map(Into::into)))
        } else {
            if matches!(
                (&setting_id, &value),
                (
                    SettingId::PresetEqualizerProfile,
                    Value::OptionalString(Some(_))
                ) | (SettingId::CustomEqualizerProfile, Value::String(_))
            ) {
                self.throttle.cancel_setting(&SettingId::VolumeAdjustments);
            }
            Action::Task(
                Task::future(async move {
                    device
                        .set_setting_values(vec![(setting_id, value)])
                        .await
                        .map_err(handle_soft_error!())?;
                    Ok(Message::RefreshSettings)
                })
                .map(coalesce_result),
            )
        }
    }

    pub fn on_key_pressed(
        &mut self,
        _modifiers: keyboard::Modifiers,
        _key: &keyboard::Key,
        _physical_key: &keyboard::key::Physical,
    ) -> Action {
        Action::None
    }
}

pub fn labeled_setting_row<'a, M>(
    label: impl Into<Cow<'a, str>> + 'a,
    element: impl Into<Element<'a, M>>,
    is_narrow: bool,
) -> Element<'a, M>
where
    M: Clone + 'static,
{
    components::setting_row(label, None, element, is_narrow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_after_midnight_conversions() {
        let midnight = MinutesAfterMidnight(0);
        assert_eq!(midnight.twelve_hour(), 12);
        assert!(!midnight.is_pm());
        assert_eq!(midnight.minute(), 0);

        let noon = MinutesAfterMidnight(12 * 60);
        assert_eq!(noon.twelve_hour(), 12);
        assert!(noon.is_pm());

        let evening = MinutesAfterMidnight(23 * 60 + 59);
        assert_eq!(evening.twelve_hour(), 11);
        assert!(evening.is_pm());
        assert_eq!(evening.minute(), 59);

        let toggled = evening.with_pm(false);
        assert_eq!(toggled.twenty_four_hour(), 11);
        assert_eq!(toggled.minute(), 59);
    }

    #[test]
    fn preset_equalizer_profile_select_retains_presets_and_equalizer() {
        let eq = settings::Equalizer {
            band_hz: Cow::Borrowed(&[100, 200, 400, 800, 1600, 3200, 6400, 12800]),
            min: -60,
            max: 60,
            fraction_digits: 1,
        };
        let select = settings::Select {
            options: vec![Cow::Borrowed("Trap"), Cow::Borrowed("Techno")],
            localized_options: vec!["Trap".to_string(), "Techno".to_string()],
        };
        let presets = vec![vec![40, 20, 0, -10, 0, 10, 20, 30], vec![30; 8]];
        let state = SettingVariantUiState::from(Setting::PresetEqualizerProfileSelect {
            equalizer: eq.clone(),
            select,
            presets: presets.clone(),
            value: Some(Cow::Borrowed("Trap")),
        });
        match state {
            SettingVariantUiState::PresetEqualizerProfileSelect {
                equalizer,
                presets: retained_presets,
                value,
                previews,
                ..
            } => {
                assert_eq!(equalizer, eq);
                assert_eq!(retained_presets, presets);
                assert_eq!(value.as_deref(), Some("Trap"));
                assert_eq!(previews.len(), 2);
                assert!(previews[0].0.is_active);
                assert!(!previews[1].0.is_active);
            }
            _ => panic!("expected PresetEqualizerProfileSelect"),
        }
    }

    #[tokio::test]
    async fn modifiable_select_add_focus_id_matches_dialog_input() {
        let config_dir = tempfile::tempdir().unwrap();
        let session =
            openscq30_lib::OpenSCQ30Session::new(config_dir.path().join("database.sqlite"))
                .await
                .unwrap();
        let mac = macaddr::MacAddr6::nil();
        session
            .pair(openscq30_lib::storage::PairedDevice {
                mac_address: mac,
                model: openscq30_lib::DeviceModel::SoundcoreA3028,
                is_demo: true,
            })
            .await
            .unwrap();
        let device = session.connect(mac).await.unwrap();
        let (mut model, _) = DeviceSettingsModel::new(
            DebugOpenSCQ30Device(device),
            session.quick_preset_handler(),
            config_dir.path().to_path_buf(),
        );

        assert!(!model.has_dialog());
        assert!(model.dialog().is_none());

        let action = model.update(Message::ShowModifiableSelectAddDialog(
            SettingId::CustomEqualizerProfile,
        ));
        let Action::FocusTextInput(focus_id) = action else {
            panic!("expected FocusTextInput action");
        };

        assert!(model.has_dialog());
        assert!(model.dialog().is_some());

        let input = DeviceSettingsModel::modifiable_select_add_text_input("");
        assert_eq!(
            cosmic::iced::advanced::Widget::<Message, cosmic::Theme, cosmic::Renderer>::id(&input),
            Some(focus_id),
        );

        let _ = model.update(Message::ModifiableSelectAddDialogSetName("Custom".into()));
        assert!(model.dismiss_dialog());
        assert!(!model.has_dialog());
        assert!(model.dialog().is_none());
    }

    #[tokio::test]
    async fn nav_select_away_from_quick_presets_closes_drawer() {
        let config_dir = tempfile::tempdir().unwrap();
        let session =
            openscq30_lib::OpenSCQ30Session::new(config_dir.path().join("database.sqlite"))
                .await
                .unwrap();
        let mac = macaddr::MacAddr6::nil();
        session
            .pair(openscq30_lib::storage::PairedDevice {
                mac_address: mac,
                model: openscq30_lib::DeviceModel::SoundcoreA3028,
                is_demo: true,
            })
            .await
            .unwrap();
        let device = session.connect(mac).await.unwrap();
        let (mut model, _) = DeviceSettingsModel::new(
            DebugOpenSCQ30Device(device),
            session.quick_preset_handler(),
            config_dir.path().to_path_buf(),
        );

        model.open_quick_preset_drawer_for_test();
        assert!(model.is_drawer_open());
        assert!(model.context_drawer().is_some());

        let equalizer_id = model
            .nav_model
            .iter()
            .find(|&id| {
                matches!(
                    model.nav_model.data::<CategoryId>(id),
                    Some(CategoryId::Equalizer)
                )
            })
            .expect("Equalizer category should exist");
        let _ = model.on_nav_select(equalizer_id);

        assert!(!model.is_drawer_open());
        assert!(!model.dismiss_drawer());
        assert!(model.context_drawer().is_none());
    }

    #[tokio::test]
    async fn preset_selection_cancels_queued_volume_adjustments() {
        let config_dir = tempfile::tempdir().unwrap();
        let session =
            openscq30_lib::OpenSCQ30Session::new(config_dir.path().join("database.sqlite"))
                .await
                .unwrap();
        let mac = macaddr::MacAddr6::nil();
        session
            .pair(openscq30_lib::storage::PairedDevice {
                mac_address: mac,
                model: openscq30_lib::DeviceModel::SoundcoreA3028,
                is_demo: true,
            })
            .await
            .unwrap();
        let device = session.connect(mac).await.unwrap();
        let (mut model, _) = DeviceSettingsModel::new(
            DebugOpenSCQ30Device(device.clone()),
            session.quick_preset_handler(),
            config_dir.path().to_path_buf(),
        );

        let initial_band_0 = match device.setting(&SettingId::VolumeAdjustments).unwrap() {
            Setting::Equalizer { value, .. } => value[0],
            other => panic!("expected Equalizer, got {other:?}"),
        };
        let staged_band_0 = initial_band_0 + 25;

        let _ = model.update(Message::SetEqualizerBand(
            SettingId::VolumeAdjustments,
            0,
            staged_band_0,
        ));
        match model
            .throttle
            .setting(&SettingId::VolumeAdjustments)
            .unwrap()
        {
            Setting::Equalizer { value, .. } => assert_eq!(value[0], staged_band_0),
            other => panic!("expected Equalizer, got {other:?}"),
        }

        let _ = model.update(Message::SetSetting(
            SettingId::PresetEqualizerProfile,
            Some(Cow::Borrowed("Rock")).into(),
        ));
        match model
            .throttle
            .setting(&SettingId::VolumeAdjustments)
            .unwrap()
        {
            Setting::Equalizer { value, .. } => assert_eq!(value[0], initial_band_0),
            other => panic!("expected Equalizer, got {other:?}"),
        }
    }
}
