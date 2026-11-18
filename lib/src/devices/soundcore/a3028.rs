use std::collections::HashMap;

use crate::devices::soundcore::{
    a3028::{packets::A3028StateUpdatePacket, state::A3028State},
    common::{
        self,
        device::fetch_state_from_state_update_packet,
        macros::soundcore_device,
        modules::{equalizer, sound_modes::AvailableSoundModes},
        packet::outbound::{RequestState, ToPacket},
        structures::{AmbientSoundMode, NoiseCancelingMode},
    },
};

mod packets;
mod state;

soundcore_device!(
    A3028State,
    async |packet_io| {
        fetch_state_from_state_update_packet::<A3028State, A3028StateUpdatePacket>(packet_io).await
    },
    async |builder| {
        builder.module_collection().add_state_update();
        builder.sound_modes(AvailableSoundModes {
            ambient_sound_modes: vec![
                AmbientSoundMode::Normal,
                AmbientSoundMode::Transparency,
                AmbientSoundMode::NoiseCanceling,
            ],
            transparency_modes: vec![],
            noise_canceling_modes: vec![
                NoiseCancelingMode::Transport,
                NoiseCancelingMode::Indoor,
                NoiseCancelingMode::Outdoor,
            ],
        });
        builder.equalizer(equalizer::common_settings()).await;
        builder.auto_power_off(
            common::modules::auto_power_off::AutoPowerOffDuration::half_hour_increments(),
        );
        builder.single_battery(5);
        builder.serial_number_and_firmware_version();
    },
    {
        HashMap::from([(
            RequestState::COMMAND,
            A3028StateUpdatePacket::default().to_packet(),
        )])
    },
);

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, collections::HashMap, sync::Arc, time::Duration};

    use macaddr::MacAddr6;
    use tokio::sync::mpsc;

    use crate::{
        api::{
            device::OpenSCQ30DeviceRegistry,
            settings::{Setting, SettingId, Value},
        },
        connection_backend::mock::rfcomm::MockRfcommBackend,
        devices::{
            DeviceModel,
            soundcore::common::{
                device::{SoundcoreDeviceConfig, test_utils::TestSoundcoreDevice},
                packet::{self, outbound::ToPacket},
                structures::{
                    AmbientSoundMode, CommonEqualizerConfiguration, CommonVolumeAdjustments,
                    NoiseCancelingMode,
                },
            },
        },
        storage::OpenSCQ30Database,
    };

    fn example_state_update_packet() -> Vec<u8> {
        vec![
            0x09, 0xff, 0x00, 0x00, 0x01, 0x01, 0x01, 0x46, 0x00, 0x05, 0x00, 0xfe, 0xfe, 0x3c,
            0xb4, 0x8f, 0xa0, 0x8e, 0xb4, 0x74, 0x88, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x02, 0x00, 0x01, 0x00, 0x30, 0x32, 0x2e, 0x33, 0x30, 0x33, 0x30, 0x32,
            0x39, 0x30, 0x38, 0x36, 0x45, 0x43, 0x38, 0x32, 0x46, 0x31, 0x32, 0x41, 0x43, 0x30,
        ]
    }

    async fn create_test_connection() -> (
        impl OpenSCQ30DeviceRegistry,
        mpsc::Sender<Vec<u8>>,
        mpsc::Receiver<Vec<u8>>,
    ) {
        let (inbound_sender, inbound_receiver) = mpsc::channel(10);
        let (outbound_sender, outbound_receiver) = mpsc::channel(10);
        let database = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let registry = super::device_registry(
            Arc::new(MockRfcommBackend::new(inbound_receiver, outbound_sender)),
            database,
            DeviceModel::SoundcoreA3028,
        );
        (registry, inbound_sender, outbound_receiver)
    }

    #[tokio::test(start_paused = true)]
    async fn test_new_with_example_state_update_packet() {
        let (registry, inbound_sender, mut outbound_receiver) = create_test_connection().await;
        inbound_sender
            .send(example_state_update_packet())
            .await
            .unwrap();
        let device = registry.connect(MacAddr6::nil()).await.unwrap();
        _ = outbound_receiver
            .recv()
            .await
            .expect("state update packet request");
        assert_eq!(
            AmbientSoundMode::Normal,
            Value::from(device.setting(&SettingId::AmbientSoundMode).unwrap())
                .try_as_enum_variant()
                .unwrap()
        );
        assert_eq!(
            NoiseCancelingMode::Transport,
            Value::from(device.setting(&SettingId::NoiseCancelingMode).unwrap())
                .try_as_enum_variant()
                .unwrap()
        );
        assert_eq!(
            None,
            Value::from(device.setting(&SettingId::PresetEqualizerProfile).unwrap())
                .try_as_optional_str()
                .unwrap()
        );
        assert_eq!(
            vec![-60, 60, 23, 40, 22, 60, -4, 16],
            Value::from(device.setting(&SettingId::VolumeAdjustments).unwrap())
                .try_into_i16_vec()
                .unwrap()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn test_new_with_retry() {
        let (registry, inbound_sender, mut outbound_receiver) = create_test_connection().await;
        tokio::spawn(async move {
            outbound_receiver
                .recv()
                .await
                .expect("state update packet request");
            tokio::time::sleep(Duration::from_millis(1500)).await;
            inbound_sender
                .send(example_state_update_packet())
                .await
                .unwrap();
        });
        registry
            .connect(MacAddr6::nil())
            .await
            .expect("should not time out");
    }

    #[tokio::test(start_paused = true)]
    async fn test_new_max_retries() {
        let (registry, _inbound_sender, _outbound_receiver) = create_test_connection().await;
        registry
            .connect(MacAddr6::nil())
            .await
            .err()
            .expect("should time out");
    }

    #[tokio::test(start_paused = true)]
    async fn test_ambient_sound_mode_update_packet() {
        let (registry, inbound_sender, mut _outbound_receiver) = create_test_connection().await;
        inbound_sender
            .send(example_state_update_packet())
            .await
            .unwrap();
        let device = registry.connect(MacAddr6::nil()).await.unwrap();

        // alert from device that sound mode changed
        inbound_sender
            .send(vec![
                0x09, 0xff, 0x00, 0x00, 0x01, 0x06, 0x01, 0x0e, 0x00, 0x00, 0x01, 0x01, 0x00, 0x20,
            ])
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;

        assert_eq!(
            AmbientSoundMode::NoiseCanceling,
            Value::from(device.setting(&SettingId::AmbientSoundMode).unwrap())
                .try_as_enum_variant()
                .unwrap()
        );
        assert_eq!(
            NoiseCancelingMode::Outdoor,
            Value::from(device.setting(&SettingId::NoiseCancelingMode).unwrap())
                .try_as_enum_variant()
                .unwrap()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn test_set_sound_mode_doesnt_resend_if_nothing_changed() {
        let (registry, inbound_sender, mut outbound_receiver) = create_test_connection().await;
        inbound_sender
            .send(example_state_update_packet())
            .await
            .unwrap();
        let device = registry.connect(MacAddr6::nil()).await.unwrap();
        outbound_receiver
            .recv()
            .await
            .expect("state update packet request");

        let f = tokio::spawn(async move {
            outbound_receiver.recv().await.expect("sound mode update");
            inbound_sender
                .send(
                    packet::outbound::SetSoundModes::COMMAND
                        .ack::<packet::InboundMarker>()
                        .bytes_with_checksum(),
                )
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
            if !outbound_receiver.is_empty() {
                let packet = outbound_receiver.recv().await.unwrap();
                panic!(
                    "another packet should not be sent since nothing has changed, but got {packet:?}"
                );
            }
        });

        // Changed sound mode
        device
            .set_setting_values(vec![(
                SettingId::AmbientSoundMode,
                Cow::from("Transparency").into(),
            )])
            .await
            .unwrap();
        // Unchanged sound mode
        device
            .set_setting_values(vec![(
                SettingId::AmbientSoundMode,
                Cow::from("Transparency").into(),
            )])
            .await
            .unwrap();
        f.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn test_set_equalizer_configuration_called_twice() {
        let (registry, inbound_sender, mut outbound_receiver) = create_test_connection().await;
        inbound_sender
            .send(example_state_update_packet())
            .await
            .unwrap();
        let device = registry.connect(MacAddr6::nil()).await.unwrap();
        outbound_receiver
            .recv()
            .await
            .expect("state update packet request");

        let f = tokio::spawn(async move {
            outbound_receiver.recv().await.expect("equalizer update");
            inbound_sender
                .send(
                    packet::outbound::SET_EQUALIZER_COMMAND
                        .ack::<packet::InboundMarker>()
                        .bytes_with_checksum(),
                )
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
            if !outbound_receiver.is_empty() {
                let packet = outbound_receiver.recv().await.unwrap();
                panic!(
                    "another packet should not be sent since nothing has changed, but got {packet:?}"
                );
            }
        });
        // Changed equalizer
        device
            .set_setting_values(vec![(
                SettingId::PresetEqualizerProfile,
                Cow::from("Acoustic").into(),
            )])
            .await
            .unwrap();

        // Unchanged equaalizer
        device
            .set_setting_values(vec![(
                SettingId::PresetEqualizerProfile,
                Cow::from("Acoustic").into(),
            )])
            .await
            .unwrap();
        f.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn test_a3028_genre_preset_outbound_packet_and_readback() {
        let state_update_packet = packet::Inbound::new(
            packet::inbound::STATE_COMMAND,
            super::A3028StateUpdatePacket::default().to_packet().body,
        );
        let mut device = TestSoundcoreDevice::new(
            super::device_registry,
            DeviceModel::SoundcoreA3028,
            HashMap::from([(packet::inbound::STATE_COMMAND, state_update_packet)]),
            SoundcoreDeviceConfig::default(),
        )
        .await;

        let cases = [
            ("Trap", [55, 30, -25, -20, 0, 15, 35, 45]),
            ("Techno", [40, 25, -15, 0, 15, 20, 30, 25]),
            ("HardTechno", [60, 40, -30, -10, 25, 35, 45, 35]),
        ];

        for (preset_name, expected_8_bands) in cases {
            device
                .assert_set_settings_response(
                    vec![(
                        SettingId::PresetEqualizerProfile,
                        Value::OptionalString(Some(preset_name.into())),
                    )],
                    vec![packet::outbound::set_equalizer(
                        &CommonEqualizerConfiguration::<1, 8>::new_all_bands_present(
                            0xfefe,
                            [CommonVolumeAdjustments::new(expected_8_bands)],
                        ),
                    )],
                )
                .await;
            device.assert_setting_values([
                (
                    SettingId::PresetEqualizerProfile,
                    Value::OptionalString(Some(preset_name.into())),
                ),
                (
                    SettingId::VolumeAdjustments,
                    expected_8_bands.to_vec().into(),
                ),
            ]);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn test_a3028_demo_genre_preset_roundtrip() {
        let database = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let registry = DeviceModel::SoundcoreA3028
            .demo_device_registry(database)
            .await
            .unwrap();
        let device = registry.connect(MacAddr6::nil()).await.unwrap();

        let Some(Setting::PresetEqualizerProfileSelect {
            select, presets, ..
        }) = device.setting(&SettingId::PresetEqualizerProfile)
        else {
            panic!("SoundcoreA3028 missing PresetEqualizerProfileSelect");
        };

        for (option, expected_curve) in select.options.iter().zip(presets.iter()) {
            device
                .set_setting_values(vec![(
                    SettingId::PresetEqualizerProfile,
                    Value::OptionalString(Some(option.clone())),
                )])
                .await
                .unwrap();
            assert_eq!(
                Value::from(device.setting(&SettingId::PresetEqualizerProfile).unwrap()),
                Value::OptionalString(Some(option.clone()))
            );
            assert_eq!(
                Value::from(device.setting(&SettingId::VolumeAdjustments).unwrap()),
                Value::I16Vec(expected_curve.clone())
            );
        }

        // Select a hardware preset, then clear PresetEqualizerProfile to None
        device
            .set_setting_values(vec![(
                SettingId::PresetEqualizerProfile,
                Value::OptionalString(Some("SoundcoreSignature".into())),
            )])
            .await
            .unwrap();
        device
            .set_setting_values(vec![(
                SettingId::PresetEqualizerProfile,
                Value::OptionalString(None),
            )])
            .await
            .unwrap();
        assert_eq!(
            Value::from(device.setting(&SettingId::PresetEqualizerProfile).unwrap()),
            Value::OptionalString(None)
        );
    }
}
