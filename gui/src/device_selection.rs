use std::{iter, sync::Arc};

use cosmic::{
    Element, Task,
    iced::{Length, alignment},
    widget,
};
use openscq30_i18n::Translate;
use openscq30_lib::{OpenSCQ30Session, storage::PairedDevice};

use crate::{
    components, fl, handle_soft_error,
    theme::{self, NARROW_BREAKPOINT, SPACE_MD, SPACE_SM, SemanticTone},
    utils::coalesce_result,
};

pub struct DeviceSelectionModel {
    paired_devices: Vec<PairedDevice>,
}

#[derive(Debug, Clone)]
pub enum Message {
    ConnectToDevice(usize),
    RemoveDevice(usize),
    AddDevice,
    SetPairedDevices(Vec<PairedDevice>),
    Warning(String),
}

pub enum Action {
    ConnectToDevice(PairedDevice),
    RemoveDevice(PairedDevice),
    AddDevice,
    None,
    Warning(String),
}

impl DeviceSelectionModel {
    pub fn new(session: Arc<OpenSCQ30Session>) -> (Self, Task<Message>) {
        let model = Self {
            paired_devices: Vec::new(),
        };
        (model, Self::refresh_paired_devices(session))
    }

    pub fn refresh_paired_devices(session: Arc<OpenSCQ30Session>) -> Task<Message> {
        Task::future(async move {
            Ok(Message::SetPairedDevices(
                session
                    .paired_devices()
                    .await
                    .map_err(handle_soft_error!())?,
            ))
        })
        .map(coalesce_result)
    }

    pub fn view(&self) -> Element<'_, Message> {
        widget::responsive(|size| {
            let is_narrow = size.width < NARROW_BREAKPOINT;
            let header = components::page_header(
                fl!("select-device"),
                Some(fl!("paired-devices-subtitle").into()),
                Some(components::primary_button(
                    fl!("add-device"),
                    Message::AddDevice,
                )),
            );

            let body: Element<'_, Message> = if self.paired_devices.is_empty() {
                components::empty_state(
                    crate::icons::openscq30(),
                    fl!("no-paired-devices"),
                    fl!("no-paired-devices-hint"),
                    None,
                )
            } else {
                widget::column(self.items(is_narrow))
                    .spacing(SPACE_MD)
                    .width(Length::Fill)
                    .into()
            };

            widget::scrollable(components::page_frame(
                widget::column(iter::once(header).chain(iter::once(body)))
                    .spacing(theme::SPACE_LG)
                    .width(Length::Fill),
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        })
        .into()
    }

    fn items(&self, is_narrow: bool) -> impl Iterator<Item = Element<'_, Message>> {
        self.paired_devices
            .iter()
            .enumerate()
            .map(move |(index, device)| {
                let mut title_items: Vec<Element<'_, Message>> =
                    vec![theme::row_title(device.model.translate()).into()];
                if device.is_demo {
                    title_items.push(components::pill_badge(
                        fl!("demo-mode").to_uppercase(),
                        SemanticTone::Accent,
                    ));
                }
                let title_row = widget::flex_row(title_items)
                    .column_spacing(SPACE_SM)
                    .row_spacing(theme::SPACE_XS)
                    .align_items(alignment::Alignment::Center);

                let model_tag = match device.model {
                    openscq30_lib::DeviceModel::AppleAirPodsPro2 => "Apple · AAP".to_owned(),
                    other => {
                        let raw = other.to_string();
                        raw.strip_prefix("Soundcore")
                            .map_or(raw.clone(), |code| format!("Soundcore · {code}"))
                    }
                };

                let icon_badge =
                    widget::container(widget::icon(crate::icons::openscq30()).size(32))
                        .align_x(alignment::Horizontal::Center)
                        .align_y(alignment::Vertical::Center);

                let info_column = widget::column![
                    title_row,
                    widget::row![
                        theme::row_subtitle(model_tag),
                        theme::mono_value(device.mac_address.to_string()),
                    ]
                    .spacing(SPACE_MD)
                    .align_y(alignment::Vertical::Center),
                ]
                .spacing(theme::SPACE_XS);

                let leading = widget::row![icon_badge, info_column.width(Length::Fill)]
                    .spacing(SPACE_MD)
                    .align_y(alignment::Vertical::Center)
                    .width(Length::Fill);

                let remove_button =
                    components::destructive_button(fl!("remove"), Message::RemoveDevice(index));
                let connect_button =
                    components::primary_button(fl!("connect"), Message::ConnectToDevice(index));

                let actions = widget::row![remove_button, connect_button]
                    .spacing(SPACE_SM)
                    .align_y(alignment::Vertical::Center);

                let content: Element<'_, Message> = if is_narrow {
                    widget::column![
                        leading,
                        widget::container(actions)
                            .width(Length::Fill)
                            .align_x(alignment::Horizontal::Right),
                    ]
                    .spacing(SPACE_MD)
                    .width(Length::Fill)
                    .into()
                } else {
                    widget::row![leading, actions]
                        .spacing(SPACE_MD)
                        .align_y(alignment::Vertical::Center)
                        .width(Length::Fill)
                        .into()
                };

                components::elevated_card(content).into()
            })
    }

    #[must_use]
    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::ConnectToDevice(index) => {
                if let Some(device) = self.paired_devices.get(index).copied() {
                    return Action::ConnectToDevice(device);
                }
            }
            Message::RemoveDevice(index) => {
                if let Some(device) = self.paired_devices.get(index).copied() {
                    return Action::RemoveDevice(device);
                }
            }
            Message::AddDevice => return Action::AddDevice,
            Message::SetPairedDevices(paired_devices) => self.paired_devices = paired_devices,
            Message::Warning(message) => return Action::Warning(message),
        }
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use macaddr::MacAddr6;
    use openscq30_lib::DeviceModel;

    use super::*;

    #[tokio::test]
    async fn update_handles_valid_and_out_of_bounds_indices_safely() {
        let config_dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            OpenSCQ30Session::new(config_dir.path().join("database.sqlite"))
                .await
                .unwrap(),
        );
        let (mut model, _) = DeviceSelectionModel::new(session);
        let device = PairedDevice {
            mac_address: MacAddr6::from([0x11, 0x22, 0x33, 0x44, 0x55, 0x66]),
            model: DeviceModel::SoundcoreA3028,
            is_demo: true,
        };
        let _ = model.update(Message::SetPairedDevices(vec![device]));

        assert!(matches!(
            model.update(Message::ConnectToDevice(0)),
            Action::ConnectToDevice(d) if d == device
        ));
        assert!(matches!(
            model.update(Message::RemoveDevice(0)),
            Action::RemoveDevice(d) if d == device
        ));
        assert!(matches!(
            model.update(Message::ConnectToDevice(5)),
            Action::None
        ));
        assert!(matches!(
            model.update(Message::RemoveDevice(5)),
            Action::None
        ));
    }
}
