use cosmic::{
    Apply, Element, Task,
    app::ContextDrawer,
    iced::{Length, alignment},
    widget,
};
use openscq30_i18n::Translate;
use openscq30_lib::{quick_presets::QuickPresetsHandler, settings, storage::QuickPreset};

use crate::{
    app::DebugOpenSCQ30Device,
    components, fl, handle_soft_error,
    theme::{self, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS},
    utils::coalesce_result,
};

#[derive(Debug, Clone)]
pub enum Message {
    Warning(String),
    RefreshQuickPresets,
    ShowCreateQuickPresetDialog,
    ActivateQuickPreset(usize),
    EditQuickPreset(usize),
    SetQuickPresets(Vec<QuickPreset>),
    CreateQuickPreset(Option<String>),
    SnapshotQuickPresetSettings(String),
    SetCreateQuickPresetName(String),
    EditQuickPresetToggleField(usize, bool),
    EditQuickPresetClose,
    EditQuickPresetModified,
    ShowDeleteQuickPresetDialog(usize),
    DeleteQuickPreset(String),
    CancelDialog,
}

pub enum Action {
    None,
    Task(Task<Message>),
    FocusTextInput(widget::Id),
    Warning(String),
}

#[derive(Debug, Clone)]
enum Dialog {
    CreateQuickPreset(String),
    DeleteQuickPreset(String),
}

#[derive(Debug)]
pub struct QuickPresetsModel {
    device: DebugOpenSCQ30Device,
    quick_presets_handler: QuickPresetsHandler,
    quick_presets: Option<Vec<QuickPreset>>,
    editing_quick_preset: Option<QuickPreset>,
    dialog: Option<Dialog>,
}

impl QuickPresetsModel {
    pub fn new(
        device: DebugOpenSCQ30Device,
        quick_presets_handler: QuickPresetsHandler,
    ) -> (Self, Task<Message>) {
        let model = Self {
            device,
            quick_presets_handler,
            quick_presets: None,
            editing_quick_preset: None,
            dialog: None,
        };
        let task = model.refresh_quick_presets();
        (model, task)
    }

    pub fn view(&self, is_narrow: bool) -> Element<'_, Message> {
        let create_btn = components::primary_button(
            fl!("create-quick-preset"),
            Message::ShowCreateQuickPresetDialog,
        );

        let header: Element<'_, Message> = if is_narrow {
            widget::column![theme::page_title(fl!("quick-presets")), create_btn]
                .spacing(SPACE_SM)
                .width(Length::Fill)
                .into()
        } else {
            components::page_header(fl!("quick-presets"), None, Some(create_btn))
        };

        let body: Element<'_, Message> = if let Some(quick_presets) = &self.quick_presets {
            if quick_presets.is_empty() {
                components::empty_state(
                    crate::icons::openscq30(),
                    fl!("no-quick-presets"),
                    fl!("no-quick-presets-hint"),
                    None,
                )
            } else {
                widget::column(quick_presets.iter().enumerate().map(|(i, preset)| {
                    let actions = widget::row![
                        components::primary_button(
                            fl!("activate"),
                            Message::ActivateQuickPreset(i)
                        ),
                        components::secondary_button(fl!("edit"), Message::EditQuickPreset(i)),
                        components::destructive_button(
                            fl!("delete"),
                            Message::ShowDeleteQuickPresetDialog(i),
                        ),
                    ]
                    .spacing(SPACE_SM)
                    .align_y(alignment::Vertical::Center);

                    let content: Element<'_, Message> = if is_narrow {
                        widget::column![
                            theme::row_title(&preset.name).width(Length::Fill),
                            widget::container(actions)
                                .width(Length::Fill)
                                .align_x(alignment::Horizontal::Right),
                        ]
                        .spacing(SPACE_MD)
                        .width(Length::Fill)
                        .into()
                    } else {
                        widget::row![theme::row_title(&preset.name).width(Length::Fill), actions]
                            .spacing(SPACE_MD)
                            .align_y(alignment::Vertical::Center)
                            .width(Length::Fill)
                            .into()
                    };

                    components::elevated_card(content).into()
                }))
                .spacing(SPACE_MD)
                .width(Length::Fill)
                .into()
            }
        } else {
            components::elevated_card(
                widget::row![
                    widget::progress_bar::indeterminate_circular().size(24.0),
                    widget::text::body(fl!("loading-item", item = fl!("quick-presets"))),
                ]
                .spacing(SPACE_MD)
                .align_y(alignment::Vertical::Center),
            )
            .into()
        };

        widget::scrollable(components::page_frame(
            widget::column![header, body]
                .spacing(SPACE_LG)
                .width(Length::Fill),
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    pub fn dialog(&self) -> Option<Element<'_, Message>> {
        self.dialog.as_ref().map(|dialog| match dialog {
            Dialog::CreateQuickPreset(name) => widget::dialog()
                .title(fl!("create-quick-preset"))
                .control(
                    widget::text_input(fl!("name"), name)
                        .id(widget::Id::new("create-quick-preset-name"))
                        .style(theme::apple_text_input())
                        .on_input(Message::SetCreateQuickPresetName)
                        .on_submit(|name| Message::CreateQuickPreset(Some(name))),
                )
                .primary_action(
                    widget::button::suggested(fl!("create"))
                        .on_press(Message::CreateQuickPreset(None)),
                )
                .secondary_action(
                    widget::button::destructive(fl!("cancel")).on_press(Message::CancelDialog),
                )
                .into(),
            Dialog::DeleteQuickPreset(name) => widget::dialog()
                .title(fl!("delete-quick-preset"))
                .body(fl!("delete-confirm", name = name))
                .primary_action(
                    widget::button::destructive(fl!("delete"))
                        .on_press(Message::DeleteQuickPreset(name.to_owned())),
                )
                .secondary_action(
                    widget::button::text(fl!("cancel")).on_press(Message::CancelDialog),
                )
                .into(),
        })
    }

    pub fn context_drawer(&self) -> Option<ContextDrawer<'_, Message>> {
        self.editing_quick_preset
            .as_ref()
            .map(|editing_quick_preset| {
                let rows: Vec<Element<'_, Message>> = editing_quick_preset
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(i, field)| {
                        widget::column![
                            widget::toggler(field.is_enabled)
                                .label(field.setting_id.translate())
                                .width(Length::Fill)
                                .spacing(SPACE_MD)
                                .on_toggle(move |enabled| {
                                    Message::EditQuickPresetToggleField(i, enabled)
                                }),
                            theme::row_subtitle(settings::localize_value(
                                self.device.setting(&field.setting_id).as_ref(),
                                &field.value,
                            )),
                        ]
                        .spacing(SPACE_XS)
                        .width(Length::Fill)
                        .into()
                    })
                    .collect();

                let content = widget::column![
                    components::inset_grouped_section(None, rows),
                    widget::button::standard(fl!("overwrite-with-current-settings"))
                        .on_press(Message::SnapshotQuickPresetSettings(
                            editing_quick_preset.name.to_owned(),
                        ))
                        .width(Length::Fill),
                ]
                .spacing(SPACE_LG)
                .padding([SPACE_SM, SPACE_MD])
                .width(Length::Fill);

                ContextDrawer {
                    title: Some(editing_quick_preset.name.as_str().into()),
                    actions: None,
                    header: None,
                    content: content.apply(Element::from),
                    footer: None,
                    on_close: Message::EditQuickPresetClose,
                }
            })
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Warning(text) => Action::Warning(text),
            Message::RefreshQuickPresets => Action::Task(self.refresh_quick_presets()),
            Message::ActivateQuickPreset(index) => {
                let Some(name) = self
                    .quick_presets
                    .as_ref()
                    .and_then(|presets| presets.get(index))
                    .map(|preset| preset.name.clone())
                else {
                    return Action::None;
                };
                let device = self.device.0.clone();
                let quick_presets_handler = self.quick_presets_handler.clone();
                Action::Task(
                    Task::future(async move {
                        quick_presets_handler
                            .activate(device.as_ref(), name)
                            .await
                            .map_err(handle_soft_error!())?;
                        Ok(Message::RefreshQuickPresets)
                    })
                    .map(coalesce_result),
                )
            }
            Message::EditQuickPreset(index) => {
                self.editing_quick_preset = self
                    .quick_presets
                    .as_ref()
                    .and_then(|presets| presets.get(index))
                    .cloned();
                Action::None
            }
            Message::SetQuickPresets(quick_presets) => {
                if let Some(editing_quick_preset) = &self.editing_quick_preset {
                    self.editing_quick_preset = quick_presets
                        .iter()
                        .find(|preset| preset.name == editing_quick_preset.name)
                        .cloned();
                }
                self.quick_presets = Some(quick_presets);
                Action::None
            }
            Message::ShowCreateQuickPresetDialog => {
                self.dialog = Some(Dialog::CreateQuickPreset(String::new()));
                Action::FocusTextInput(widget::Id::new("create-quick-preset-name"))
            }
            Message::SetCreateQuickPresetName(name) => {
                self.dialog = Some(Dialog::CreateQuickPreset(name));
                Action::None
            }
            Message::CreateQuickPreset(override_name) => {
                let Some(Dialog::CreateQuickPreset(name)) = self.dialog.take() else {
                    return Action::None;
                };
                let name = override_name.unwrap_or(name);

                let device = self.device.clone();
                let quick_presets_handler = self.quick_presets_handler.clone();
                Action::Task(
                    Task::future(async move {
                        quick_presets_handler
                            .save(device.as_ref(), name)
                            .await
                            .map_err(handle_soft_error!())?;
                        Ok(Message::RefreshQuickPresets)
                    })
                    .map(coalesce_result),
                )
            }
            Message::ShowDeleteQuickPresetDialog(index) => {
                if let Some(quick_presets) = &self.quick_presets {
                    self.dialog = quick_presets
                        .get(index)
                        .map(|preset| Dialog::DeleteQuickPreset(preset.name.to_owned()));
                }
                Action::None
            }
            Message::DeleteQuickPreset(name) => {
                self.dialog = None;
                let quick_presets_handler = self.quick_presets_handler.clone();
                let device = self.device.clone();
                Action::Task(
                    Task::future(async move {
                        quick_presets_handler
                            .delete(device.as_ref(), name)
                            .await
                            .map_err(handle_soft_error!())?;
                        Ok(Message::RefreshQuickPresets)
                    })
                    .map(coalesce_result),
                )
            }
            Message::SnapshotQuickPresetSettings(name) => {
                let device = self.device.clone();
                let quick_presets_handler = self.quick_presets_handler.clone();
                Action::Task(
                    Task::future(async move {
                        quick_presets_handler
                            .save(device.as_ref(), name)
                            .await
                            .map_err(handle_soft_error!())?;
                        Ok(Message::RefreshQuickPresets)
                    })
                    .map(coalesce_result),
                )
            }
            Message::EditQuickPresetToggleField(field_index, is_enabled) => {
                if let Some(preset) = &mut self.editing_quick_preset {
                    let preset_name = preset.name.to_owned();

                    // eagerly update the displayed value. ideally we would revert back to
                    // what it was if the returned future fails.
                    let Some(field) = preset.fields.get_mut(field_index) else {
                        return Action::None;
                    };
                    field.is_enabled = is_enabled;

                    let setting_id = field.setting_id;
                    let device = self.device.clone();
                    let quick_presets_handler = self.quick_presets_handler.clone();
                    return Action::Task(
                        Task::future(async move {
                            quick_presets_handler
                                .toggle_field(device.as_ref(), preset_name, setting_id, is_enabled)
                                .await
                                .map_err(handle_soft_error!())?;
                            Ok(Message::EditQuickPresetModified)
                        })
                        .map(coalesce_result),
                    );
                }
                Action::None
            }
            Message::EditQuickPresetClose => {
                self.editing_quick_preset = None;
                Action::None
            }
            Message::EditQuickPresetModified => {
                // TODO either modify in place instead of re-fetching all presets, or fetch only the modified preset
                let quick_presets_handler = self.quick_presets_handler.clone();
                let device = self.device.clone();
                Action::Task(
                    Task::future(async move {
                        quick_presets_handler
                            .quick_presets(device.as_ref())
                            .await
                            .map(Message::SetQuickPresets)
                            .map_err(handle_soft_error!())
                    })
                    .map(coalesce_result),
                )
            }
            Message::CancelDialog => {
                self.close_dialog();
                Action::None
            }
        }
    }

    fn refresh_quick_presets(&self) -> Task<Message> {
        let device = self.device.clone();
        let quick_presets_handler = self.quick_presets_handler.clone();
        Task::future(async move {
            quick_presets_handler
                .quick_presets(device.as_ref())
                .await
                .map(Message::SetQuickPresets)
                .map_err(handle_soft_error!())
        })
        .map(coalesce_result)
    }

    pub fn has_dialog(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn close_dialog(&mut self) -> bool {
        if self.dialog.is_some() {
            self.dialog = None;
            true
        } else {
            false
        }
    }

    pub fn is_drawer_open(&self) -> bool {
        self.editing_quick_preset.is_some()
    }

    pub fn close_drawer(&mut self) -> bool {
        if self.editing_quick_preset.is_some() {
            self.editing_quick_preset = None;
            true
        } else {
            false
        }
    }
}
