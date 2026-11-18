use std::sync::Arc;

use openscq30_lib_has::Has;
use setting_handler::EqualizerSettingHandler;
use state_modifier::{
    EqualizerStateModifier, EqualizerStateModifierOptions, EqualizerWithBasicHearIdStateModifier,
    EqualizerWithCustomHearIdStateModifier,
};
use strum::{EnumIter, EnumString, IntoStaticStr};
use tokio::sync::watch;

use crate::{
    api::settings::{CategoryId, SettingId},
    devices::{
        DeviceModel,
        soundcore::common::{
            modules::equalizer::{
                custom_equalizer_profile_store::CustomEqualizerProfileStore,
                import_export_setting_handler::ImportExportSettingHandler,
                state_modifier::EqualizerWithCustomHearIdOptions,
            },
            packet::PacketIOController,
            state_modifier::StateModifier,
            structures::{
                AgeRange, BasicHearId, CustomHearId, EqualizerConfiguration, Gender, TwsStatus,
                VolumeAdjustments,
            },
        },
    },
    i18n::fl,
    macros::enum_subset,
    storage::OpenSCQ30Database,
};

use super::ModuleCollection;

mod custom_equalizer_profile_store;
mod import_export_setting_handler;
mod setting_handler;
mod state_modifier;

enum_subset!(
    SettingId,
    #[derive(Eq, PartialEq, EnumString, EnumIter, IntoStaticStr)]
    enum EqualizerSetting {
        PresetEqualizerProfile,
        CustomEqualizerProfile,
        VolumeAdjustments,
    }
);

enum_subset!(
    SettingId,
    #[derive(EnumString, EnumIter, IntoStaticStr)]
    enum ImportExportSetting {
        ImportCustomEqualizerProfiles,
        ExportCustomEqualizerProfiles,
        ExportCustomEqualizerProfilesOutput,
    }
);

/// There are situations where there are 10 equalizer bands, but presets only assign 9 of them, and only 8 of them are
/// visible. To suit those requirements, all three band numbers are separate. VISIBLE_BANDS <= PRESET_BANDS <= BANDS is
/// const asserted since it is likely an error, but it is possible a device could violate this assumption in the future.
///
/// This is generic over min/max volume and number of fractional digits because it makes it impossible to mix up structs
/// that aren't compatible with eachother.
pub struct EqualizerModuleSettings<
    const VISIBLE_BANDS: usize,
    const PRESET_BANDS: usize,
    const MIN_VOLUME: i16,
    const MAX_VOLUME: i16,
    const FRACTION_DIGITS: u8,
> {
    pub custom_preset_id: Option<u16>,
    pub band_hz: [u16; VISIBLE_BANDS],
    pub presets: Vec<EqualizerPreset<PRESET_BANDS, MIN_VOLUME, MAX_VOLUME, FRACTION_DIGITS>>,
    pub preset_min_volume: i16,
    pub preset_max_volume: i16,
    pub invisible_bands_mode: InvisibleBandsMode,
}

pub enum InvisibleBandsMode {
    Fixed(Vec<i16>),
}

#[derive(Clone, Copy, Debug)]
pub struct EqualizerPreset<
    const BANDS: usize,
    const MIN_VOLUME: i16,
    const MAX_VOLUME: i16,
    const FRACTION_DIGITS: u8,
> {
    pub id: u16,
    pub name: &'static str,
    pub localized_name: fn() -> String,
    pub volume_adjustments: VolumeAdjustments<BANDS, MIN_VOLUME, MAX_VOLUME, FRACTION_DIGITS>,
}

pub const COMMON_VISIBLE_BANDS: usize = 8;
pub const COMMON_PRESET_BANDS: usize = 10;
pub const COMMON_MIN_VOLUME: i16 = -120;
pub const COMMON_MAX_VOLUME: i16 = 134;
pub const COMMON_FRACTION_DIGITS: u8 = 1;
pub const COMMON_BAND_HZ: [u16; COMMON_VISIBLE_BANDS] =
    [100, 200, 400, 800, 1600, 3200, 6400, 12800];

pub fn common_settings() -> EqualizerModuleSettings<
    COMMON_VISIBLE_BANDS,
    COMMON_PRESET_BANDS,
    COMMON_MIN_VOLUME,
    COMMON_MAX_VOLUME,
    COMMON_FRACTION_DIGITS,
> {
    common_settings_with_presets(vec![
        EqualizerPreset {
            name: "SoundcoreSignature",
            localized_name: || fl!("soundcore-signature"),
            id: 0x0000,
            volume_adjustments: VolumeAdjustments::new([0, 0, 0, 0, 0, 0, 0, 0, 0, -120]),
        },
        EqualizerPreset {
            name: "Acoustic",
            localized_name: || fl!("acoustic"),
            id: 0x0001,
            volume_adjustments: VolumeAdjustments::new([40, 10, 20, 20, 40, 40, 40, 20, 0, -120]),
        },
        EqualizerPreset {
            name: "BassBooster",
            localized_name: || fl!("bass-booster"),
            id: 0x0002,
            volume_adjustments: VolumeAdjustments::new([40, 30, 10, 0, 0, 0, 0, 0, 0, -120]),
        },
        EqualizerPreset {
            name: "BassReducer",
            localized_name: || fl!("bass-reducer"),
            id: 0x0003,
            volume_adjustments: VolumeAdjustments::new([-40, -30, -10, 0, 0, 0, 0, 0, 0, -120]),
        },
        EqualizerPreset {
            name: "Classical",
            localized_name: || fl!("classical"),
            id: 0x0004,
            volume_adjustments: VolumeAdjustments::new([30, 30, -20, -20, 0, 20, 30, 40, 0, -120]),
        },
        EqualizerPreset {
            name: "Podcast",
            localized_name: || fl!("podcast"),
            id: 0x0005,
            volume_adjustments: VolumeAdjustments::new([-30, 20, 40, 40, 30, 20, 0, -20, 0, -120]),
        },
        EqualizerPreset {
            name: "Dance",
            localized_name: || fl!("dance"),
            id: 0x0006,
            volume_adjustments: VolumeAdjustments::new([
                20, -30, -10, 10, 20, 20, 10, -30, 0, -120,
            ]),
        },
        EqualizerPreset {
            name: "Deep",
            localized_name: || fl!("deep"),
            id: 0x0007,
            volume_adjustments: VolumeAdjustments::new([
                20, 10, 30, 30, 20, -20, -40, -50, 0, -120,
            ]),
        },
        EqualizerPreset {
            name: "Electronic",
            localized_name: || fl!("electronic"),
            id: 0x0008,
            volume_adjustments: VolumeAdjustments::new([30, 20, -20, 20, 10, 20, 30, 30, 0, -120]),
        },
        EqualizerPreset {
            name: "Flat",
            localized_name: || fl!("flat"),
            id: 0x0009,
            volume_adjustments: VolumeAdjustments::new([-20, -20, -10, 0, 0, 0, -20, -20, 0, -120]),
        },
        EqualizerPreset {
            name: "HipHop",
            localized_name: || fl!("hip-hop"),
            id: 0x000a,
            volume_adjustments: VolumeAdjustments::new([
                20, 30, -10, -10, 20, -10, 20, 30, 0, -120,
            ]),
        },
        EqualizerPreset {
            name: "Jazz",
            localized_name: || fl!("jazz"),
            id: 0x000b,
            volume_adjustments: VolumeAdjustments::new([20, 20, -20, -20, 0, 20, 30, 40, 0, -120]),
        },
        EqualizerPreset {
            name: "Latin",
            localized_name: || fl!("latin"),
            id: 0x000c,
            volume_adjustments: VolumeAdjustments::new([0, 0, -20, -20, -20, 0, 30, 50, 0, -120]),
        },
        EqualizerPreset {
            name: "Lounge",
            localized_name: || fl!("lounge"),
            id: 0x000d,
            volume_adjustments: VolumeAdjustments::new([-10, 20, 40, 30, 0, -20, 20, 10, 0, -120]),
        },
        EqualizerPreset {
            name: "Piano",
            localized_name: || fl!("piano"),
            id: 0x000e,
            volume_adjustments: VolumeAdjustments::new([0, 30, 30, 20, 40, 50, 30, 40, 0, -120]),
        },
        EqualizerPreset {
            name: "Pop",
            localized_name: || fl!("pop"),
            id: 0x000f,
            volume_adjustments: VolumeAdjustments::new([
                -10, 10, 30, 30, 10, -10, -20, -30, 0, -120,
            ]),
        },
        EqualizerPreset {
            name: "RnB",
            localized_name: || fl!("rnb"),
            id: 0x0010,
            volume_adjustments: VolumeAdjustments::new([60, 20, -20, -20, 20, 30, 30, 40, 0, -120]),
        },
        EqualizerPreset {
            name: "Rock",
            localized_name: || fl!("rock"),
            id: 0x0011,
            volume_adjustments: VolumeAdjustments::new([30, 20, -10, -10, 10, 30, 30, 30, 0, -120]),
        },
        EqualizerPreset {
            name: "SmallSpeakers",
            localized_name: || fl!("small-speakers"),
            id: 0x0012,
            volume_adjustments: VolumeAdjustments::new([
                40, 30, 10, 0, -20, -30, -40, -40, 0, -120,
            ]),
        },
        EqualizerPreset {
            name: "SpokenWord",
            localized_name: || fl!("spoken-word"),
            id: 0x0013,
            volume_adjustments: VolumeAdjustments::new([-30, -20, 10, 20, 20, 10, 0, -30, 0, -120]),
        },
        EqualizerPreset {
            name: "TrebleBooster",
            localized_name: || fl!("treble-booster"),
            id: 0x0014,
            volume_adjustments: VolumeAdjustments::new([
                -20, -20, -20, -10, 10, 20, 20, 40, 0, -120,
            ]),
        },
        EqualizerPreset {
            name: "TrebleReducer",
            localized_name: || fl!("treble-reducer"),
            id: 0x0015,
            volume_adjustments: VolumeAdjustments::new([0, 0, 0, -20, -30, -40, -40, -60, 0, -120]),
        },
    ])
}

pub fn common_settings_type_2() -> EqualizerModuleSettings<8, 10, -120, 134, 1> {
    common_settings_with_presets(vec![
        EqualizerPreset {
            name: "SoundcoreSignature",
            localized_name: || fl!("soundcore-signature"),
            id: 0,
            volume_adjustments: VolumeAdjustments::new([0, 0, 0, 0, 0, 0, 0, 0, 0, -120]),
        },
        EqualizerPreset {
            name: "Acoustic",
            localized_name: || fl!("acoustic"),
            id: 1,
            volume_adjustments: VolumeAdjustments::new([40, 10, 20, 20, 40, 40, 40, 20, 0, -120]),
        },
        EqualizerPreset {
            name: "BassBooster",
            localized_name: || fl!("bass-booster"),
            id: 2,
            volume_adjustments: VolumeAdjustments::new([40, 30, 10, 0, 0, 0, 0, 0, 0, -120]),
        },
        EqualizerPreset {
            name: "BassReducer",
            localized_name: || fl!("bass-reducer"),
            id: 3,
            volume_adjustments: VolumeAdjustments::new([-40, -30, -10, 0, 0, 0, 0, 0, 0, -120]),
        },
        EqualizerPreset {
            name: "Classical",
            localized_name: || fl!("classical"),
            id: 4,
            volume_adjustments: VolumeAdjustments::new([30, 30, -20, -20, 0, 20, 30, 40, 0, -120]),
        },
        EqualizerPreset {
            name: "Podcast",
            localized_name: || fl!("podcast"),
            id: 5,
            volume_adjustments: VolumeAdjustments::new([-30, 20, 40, 40, 30, 20, 0, -20, 0, -120]),
        },
        EqualizerPreset {
            name: "Dance",
            localized_name: || fl!("dance"),
            id: 6,
            volume_adjustments: VolumeAdjustments::new([
                20, -30, -10, 10, 20, 20, 10, -30, 0, -120,
            ]),
        },
        EqualizerPreset {
            name: "Deep",
            localized_name: || fl!("deep"),
            id: 7,
            volume_adjustments: VolumeAdjustments::new([
                20, 10, 30, 30, 20, -20, -40, -50, 0, -120,
            ]),
        },
        EqualizerPreset {
            name: "Electronic",
            localized_name: || fl!("electronic"),
            id: 8,
            volume_adjustments: VolumeAdjustments::new([30, 20, -20, 20, 10, 20, 30, 30, 0, -120]),
        },
        EqualizerPreset {
            name: "Flat",
            localized_name: || fl!("flat"),
            id: 9,
            volume_adjustments: VolumeAdjustments::new([-20, -20, -10, 0, 0, 0, -20, -20, 0, -120]),
        },
        EqualizerPreset {
            name: "HipHop",
            localized_name: || fl!("hip-hop"),
            id: 10,
            volume_adjustments: VolumeAdjustments::new([
                20, 30, -10, -10, 20, -10, 20, 30, 0, -120,
            ]),
        },
        EqualizerPreset {
            name: "Jazz",
            localized_name: || fl!("jazz"),
            id: 11,
            volume_adjustments: VolumeAdjustments::new([20, 20, -20, -20, 0, 20, 30, 40, 0, -120]),
        },
        EqualizerPreset {
            name: "Latin",
            localized_name: || fl!("latin"),
            id: 12,
            volume_adjustments: VolumeAdjustments::new([0, 0, -20, -20, -20, 0, 30, 50, 0, -120]),
        },
        EqualizerPreset {
            name: "Lounge",
            localized_name: || fl!("lounge"),
            id: 13,
            volume_adjustments: VolumeAdjustments::new([-10, 20, 40, 30, 0, -20, 20, 10, 0, -120]),
        },
        EqualizerPreset {
            name: "Piano",
            localized_name: || fl!("piano"),
            id: 14,
            volume_adjustments: VolumeAdjustments::new([0, 30, 30, 20, 40, 50, 30, 40, 0, -120]),
        },
        EqualizerPreset {
            name: "Pop",
            localized_name: || fl!("pop"),
            id: 15,
            volume_adjustments: VolumeAdjustments::new([
                -10, 10, 30, 30, 10, -10, -20, -30, 0, -120,
            ]),
        },
        EqualizerPreset {
            name: "RnB",
            localized_name: || fl!("rnb"),
            id: 16,
            volume_adjustments: VolumeAdjustments::new([60, 20, -20, -20, 20, 30, 30, 40, 0, -120]),
        },
        EqualizerPreset {
            name: "Rock",
            localized_name: || fl!("rock"),
            id: 17,
            volume_adjustments: VolumeAdjustments::new([30, 20, -10, -10, 10, 30, 40, 50, 0, -120]),
        },
        EqualizerPreset {
            name: "SmallSpeakers",
            localized_name: || fl!("small-speakers"),
            id: 18,
            volume_adjustments: VolumeAdjustments::new([
                40, 30, 10, 0, -20, -30, -40, -40, 0, -120,
            ]),
        },
        EqualizerPreset {
            name: "SpokenWord",
            localized_name: || fl!("spoken-word"),
            id: 19,
            volume_adjustments: VolumeAdjustments::new([-30, -20, 10, 20, 20, 10, 0, -30, 0, -120]),
        },
        EqualizerPreset {
            name: "TrebleBooster",
            localized_name: || fl!("treble-booster"),
            id: 20,
            volume_adjustments: VolumeAdjustments::new([
                -20, -20, -20, -10, 10, 20, 20, 40, 0, -120,
            ]),
        },
        EqualizerPreset {
            name: "TrebleReducer",
            localized_name: || fl!("treble-reducer"),
            id: 21,
            volume_adjustments: VolumeAdjustments::new([0, 0, 0, -20, -30, -40, -40, -60, 0, -120]),
        },
    ])
}

pub fn common_settings_with_presets<const PRESET_BANDS: usize, const FRACTION_DIGITS: u8>(
    presets: Vec<
        EqualizerPreset<PRESET_BANDS, COMMON_MIN_VOLUME, COMMON_MAX_VOLUME, FRACTION_DIGITS>,
    >,
) -> EqualizerModuleSettings<
    COMMON_VISIBLE_BANDS,
    PRESET_BANDS,
    COMMON_MIN_VOLUME,
    COMMON_MAX_VOLUME,
    FRACTION_DIGITS,
> {
    EqualizerModuleSettings {
        custom_preset_id: Some(0xfefe),
        band_hz: COMMON_BAND_HZ,
        invisible_bands_mode: InvisibleBandsMode::Fixed(vec![0, 0]),
        preset_min_volume: -60,
        preset_max_volume: 60,
        presets,
    }
}

pub fn common_genre_presets() -> Vec<
    EqualizerPreset<
        COMMON_PRESET_BANDS,
        COMMON_MIN_VOLUME,
        COMMON_MAX_VOLUME,
        COMMON_FRACTION_DIGITS,
    >,
> {
    vec![
        EqualizerPreset {
            name: "Trap",
            localized_name: || fl!("trap"),
            id: 0xfefe,
            volume_adjustments: VolumeAdjustments::new([55, 30, -25, -20, 0, 15, 35, 45, 0, -120]),
        },
        EqualizerPreset {
            name: "Techno",
            localized_name: || fl!("techno"),
            id: 0xfefe,
            volume_adjustments: VolumeAdjustments::new([40, 25, -15, 0, 15, 20, 30, 25, 0, -120]),
        },
        EqualizerPreset {
            name: "HardTechno",
            localized_name: || fl!("hard-techno"),
            id: 0xfefe,
            volume_adjustments: VolumeAdjustments::new([60, 40, -30, -10, 25, 35, 45, 35, 0, -120]),
        },
        EqualizerPreset {
            name: "House",
            localized_name: || fl!("house"),
            id: 0xfefe,
            volume_adjustments: VolumeAdjustments::new([35, 30, -10, 10, 15, 10, 25, 30, 0, -120]),
        },
        EqualizerPreset {
            name: "DrumAndBass",
            localized_name: || fl!("drum-and-bass"),
            id: 0xfefe,
            volume_adjustments: VolumeAdjustments::new([50, 20, -20, -10, 10, 30, 40, 40, 0, -120]),
        },
        EqualizerPreset {
            name: "Dubstep",
            localized_name: || fl!("dubstep"),
            id: 0xfefe,
            volume_adjustments: VolumeAdjustments::new([60, 35, -30, 15, 30, 25, 35, 20, 0, -120]),
        },
        EqualizerPreset {
            name: "Metal",
            localized_name: || fl!("metal"),
            id: 0xfefe,
            volume_adjustments: VolumeAdjustments::new([35, 15, -30, -25, 15, 40, 45, 25, 0, -120]),
        },
        EqualizerPreset {
            name: "LoFi",
            localized_name: || fl!("lo-fi"),
            id: 0xfefe,
            volume_adjustments: VolumeAdjustments::new([
                20, 30, 20, 10, -10, -25, -40, -50, 0, -120,
            ]),
        },
        EqualizerPreset {
            name: "Indie",
            localized_name: || fl!("indie"),
            id: 0xfefe,
            volume_adjustments: VolumeAdjustments::new([10, 15, -10, 15, 25, 25, 20, 15, 0, -120]),
        },
        EqualizerPreset {
            name: "Reggae",
            localized_name: || fl!("reggae"),
            id: 0xfefe,
            volume_adjustments: VolumeAdjustments::new([50, 40, 10, -15, -10, 20, 25, 10, 0, -120]),
        },
        EqualizerPreset {
            name: "Ambient",
            localized_name: || fl!("ambient"),
            id: 0xfefe,
            volume_adjustments: VolumeAdjustments::new([25, 15, -15, -10, 5, 15, 30, 40, 0, -120]),
        },
        EqualizerPreset {
            name: "Funk",
            localized_name: || fl!("funk"),
            id: 0xfefe,
            volume_adjustments: VolumeAdjustments::new([30, 35, -15, 0, 20, 30, 30, 20, 0, -120]),
        },
        EqualizerPreset {
            name: "Country",
            localized_name: || fl!("country"),
            id: 0xfefe,
            volume_adjustments: VolumeAdjustments::new([15, 20, 0, 20, 30, 25, 20, 10, 0, -120]),
        },
    ]
}

impl<T: 'static> ModuleCollection<T> {
    pub async fn add_equalizer<
        const CHANNELS: usize,
        const BANDS: usize,
        const VISIBLE_BANDS: usize,
        const PRESET_BANDS: usize,
        const MIN_VOLUME: i16,
        const MAX_VOLUME: i16,
        const FRACTION_DIGITS: u8,
    >(
        &mut self,
        packet_io: Arc<PacketIOController>,
        database: Arc<OpenSCQ30Database>,
        device_model: DeviceModel,
        change_notify: watch::Sender<()>,
        settings: EqualizerModuleSettings<
            VISIBLE_BANDS,
            PRESET_BANDS,
            MIN_VOLUME,
            MAX_VOLUME,
            FRACTION_DIGITS,
        >,
    ) where
        T: Has<EqualizerConfiguration<CHANNELS, BANDS, MIN_VOLUME, MAX_VOLUME, FRACTION_DIGITS>>
            + Clone
            + Send
            + Sync,
    {
        self.add_equalizer_with_custom_state_modifier(
            database,
            device_model,
            change_notify,
            Box::new(EqualizerStateModifier::new(
                packet_io,
                EqualizerStateModifierOptions { has_drc: false },
            )),
            settings,
        )
        .await;
    }

    pub async fn add_equalizer_tws<
        const CHANNELS: usize,
        const BANDS: usize,
        const VISIBLE_BANDS: usize,
        const PRESET_BANDS: usize,
        const MIN_VOLUME: i16,
        const MAX_VOLUME: i16,
        const FRACTION_DIGITS: u8,
    >(
        &mut self,
        packet_io: Arc<PacketIOController>,
        database: Arc<OpenSCQ30Database>,
        device_model: DeviceModel,
        change_notify: watch::Sender<()>,
        settings: EqualizerModuleSettings<
            VISIBLE_BANDS,
            PRESET_BANDS,
            MIN_VOLUME,
            MAX_VOLUME,
            FRACTION_DIGITS,
        >,
    ) where
        T: Has<EqualizerConfiguration<CHANNELS, BANDS, MIN_VOLUME, MAX_VOLUME, FRACTION_DIGITS>>
            + Has<TwsStatus>
            + Clone
            + Send
            + Sync,
    {
        self.add_equalizer_with_custom_state_modifier_tws(
            database,
            device_model,
            change_notify,
            Box::new(EqualizerStateModifier::new(
                packet_io,
                EqualizerStateModifierOptions { has_drc: false },
            )),
            settings,
        )
        .await;
    }

    pub async fn add_equalizer_with_drc<
        const CHANNELS: usize,
        const BANDS: usize,
        const VISIBLE_BANDS: usize,
        const PRESET_BANDS: usize,
        const MIN_VOLUME: i16,
        const MAX_VOLUME: i16,
        const FRACTION_DIGITS: u8,
    >(
        &mut self,
        packet_io: Arc<PacketIOController>,
        database: Arc<OpenSCQ30Database>,
        device_model: DeviceModel,
        change_notify: watch::Sender<()>,
        settings: EqualizerModuleSettings<
            VISIBLE_BANDS,
            PRESET_BANDS,
            MIN_VOLUME,
            MAX_VOLUME,
            FRACTION_DIGITS,
        >,
    ) where
        T: Has<EqualizerConfiguration<CHANNELS, BANDS, MIN_VOLUME, MAX_VOLUME, FRACTION_DIGITS>>
            + Clone
            + Send
            + Sync,
    {
        self.add_equalizer_with_custom_state_modifier(
            database,
            device_model,
            change_notify,
            Box::new(EqualizerStateModifier::new(
                packet_io,
                EqualizerStateModifierOptions { has_drc: true },
            )),
            settings,
        )
        .await;
    }

    pub async fn add_equalizer_with_drc_tws<
        const CHANNELS: usize,
        const BANDS: usize,
        const VISIBLE_BANDS: usize,
        const PRESET_BANDS: usize,
        const MIN_VOLUME: i16,
        const MAX_VOLUME: i16,
        const FRACTION_DIGITS: u8,
    >(
        &mut self,
        packet_io: Arc<PacketIOController>,
        database: Arc<OpenSCQ30Database>,
        device_model: DeviceModel,
        change_notify: watch::Sender<()>,
        settings: EqualizerModuleSettings<
            VISIBLE_BANDS,
            PRESET_BANDS,
            MIN_VOLUME,
            MAX_VOLUME,
            FRACTION_DIGITS,
        >,
    ) where
        T: Has<EqualizerConfiguration<CHANNELS, BANDS, MIN_VOLUME, MAX_VOLUME, FRACTION_DIGITS>>
            + Has<TwsStatus>
            + Clone
            + Send
            + Sync,
    {
        self.add_equalizer_with_custom_state_modifier_tws(
            database,
            device_model,
            change_notify,
            Box::new(EqualizerStateModifier::new(
                packet_io,
                EqualizerStateModifierOptions { has_drc: true },
            )),
            settings,
        )
        .await;
    }

    pub async fn add_equalizer_with_basic_hear_id_tws<
        const CHANNELS: usize,
        const BANDS: usize,
        const VISIBLE_BANDS: usize,
        const PRESET_BANDS: usize,
        const MIN_VOLUME: i16,
        const MAX_VOLUME: i16,
        const FRACTION_DIGITS: u8,
    >(
        &mut self,
        packet_io: Arc<PacketIOController>,
        database: Arc<OpenSCQ30Database>,
        device_model: DeviceModel,
        change_notify: watch::Sender<()>,
        settings: EqualizerModuleSettings<
            VISIBLE_BANDS,
            PRESET_BANDS,
            MIN_VOLUME,
            MAX_VOLUME,
            FRACTION_DIGITS,
        >,
    ) where
        T: Has<EqualizerConfiguration<CHANNELS, BANDS, MIN_VOLUME, MAX_VOLUME, FRACTION_DIGITS>>
            + Has<TwsStatus>
            + Has<BasicHearId<CHANNELS, BANDS>>
            + Has<Gender>
            + Has<AgeRange>
            + Clone
            + Send
            + Sync,
    {
        self.add_equalizer_with_custom_state_modifier_tws(
            database,
            device_model,
            change_notify,
            Box::new(EqualizerWithBasicHearIdStateModifier::new(packet_io)),
            settings,
        )
        .await;
    }

    pub async fn add_equalizer_with_custom_hear_id_tws<
        const CHANNELS: usize,
        const BANDS: usize,
        const VISIBLE_BANDS: usize,
        const PRESET_BANDS: usize,
        const MIN_VOLUME: i16,
        const MAX_VOLUME: i16,
        const FRACTION_DIGITS: u8,
    >(
        &mut self,
        packet_io: Arc<PacketIOController>,
        database: Arc<OpenSCQ30Database>,
        device_model: DeviceModel,
        change_notify: watch::Sender<()>,
        settings: EqualizerModuleSettings<
            VISIBLE_BANDS,
            PRESET_BANDS,
            MIN_VOLUME,
            MAX_VOLUME,
            FRACTION_DIGITS,
        >,
    ) where
        T: Has<EqualizerConfiguration<CHANNELS, BANDS, MIN_VOLUME, MAX_VOLUME, FRACTION_DIGITS>>
            + Has<TwsStatus>
            + Has<CustomHearId<CHANNELS, BANDS>>
            + Has<Gender>
            + Has<AgeRange>
            + Clone
            + Send
            + Sync,
    {
        self.add_equalizer_with_custom_state_modifier_tws(
            database,
            device_model,
            change_notify,
            Box::new(EqualizerWithCustomHearIdStateModifier::new(
                packet_io,
                EqualizerWithCustomHearIdOptions {
                    force_supports_hear_id: false,
                },
            )),
            settings,
        )
        .await;
    }

    pub async fn add_equalizer_with_custom_hear_id_tws_force_supports_hear_id<
        const CHANNELS: usize,
        const BANDS: usize,
        const VISIBLE_BANDS: usize,
        const PRESET_BANDS: usize,
        const MIN_VOLUME: i16,
        const MAX_VOLUME: i16,
        const FRACTION_DIGITS: u8,
    >(
        &mut self,
        packet_io: Arc<PacketIOController>,
        database: Arc<OpenSCQ30Database>,
        device_model: DeviceModel,
        change_notify: watch::Sender<()>,
        settings: EqualizerModuleSettings<
            VISIBLE_BANDS,
            PRESET_BANDS,
            MIN_VOLUME,
            MAX_VOLUME,
            FRACTION_DIGITS,
        >,
    ) where
        T: Has<EqualizerConfiguration<CHANNELS, BANDS, MIN_VOLUME, MAX_VOLUME, FRACTION_DIGITS>>
            + Has<TwsStatus>
            + Has<CustomHearId<CHANNELS, BANDS>>
            + Has<Gender>
            + Has<AgeRange>
            + Clone
            + Send
            + Sync,
    {
        self.add_equalizer_with_custom_state_modifier_tws(
            database,
            device_model,
            change_notify,
            Box::new(EqualizerWithCustomHearIdStateModifier::new(
                packet_io,
                EqualizerWithCustomHearIdOptions {
                    force_supports_hear_id: true,
                },
            )),
            settings,
        )
        .await;
    }

    pub async fn add_equalizer_with_custom_state_modifier_tws<
        const CHANNELS: usize,
        const BANDS: usize,
        const VISIBLE_BANDS: usize,
        const PRESET_BANDS: usize,
        const MIN_VOLUME: i16,
        const MAX_VOLUME: i16,
        const FRACTION_DIGITS: u8,
    >(
        &mut self,
        database: Arc<OpenSCQ30Database>,
        device_model: DeviceModel,
        change_notify: watch::Sender<()>,
        state_modifier: Box<dyn StateModifier<T> + Send + Sync + 'static>,
        module_settings: EqualizerModuleSettings<
            VISIBLE_BANDS,
            PRESET_BANDS,
            MIN_VOLUME,
            MAX_VOLUME,
            FRACTION_DIGITS,
        >,
    ) where
        T: Has<EqualizerConfiguration<CHANNELS, BANDS, MIN_VOLUME, MAX_VOLUME, FRACTION_DIGITS>>
            + Has<TwsStatus>
            + Clone
            + Send
            + Sync,
    {
        let profile_store = Arc::new(
            CustomEqualizerProfileStore::new(database, device_model, change_notify.to_owned())
                .await,
        );
        self.setting_manager.add_handler(
            CategoryId::Equalizer,
            EqualizerSettingHandler::<
                T,
                CHANNELS,
                BANDS,
                VISIBLE_BANDS,
                PRESET_BANDS,
                MIN_VOLUME,
                MAX_VOLUME,
                FRACTION_DIGITS,
            >::new(profile_store.to_owned(), module_settings)
            .with_tws(),
        );
        self.setting_manager.add_handler(
            CategoryId::EqualizerImportExport,
            ImportExportSettingHandler::new(profile_store, change_notify),
        );
        self.state_modifiers.push(state_modifier);
    }

    pub async fn add_equalizer_with_custom_state_modifier<
        const CHANNELS: usize,
        const BANDS: usize,
        const VISIBLE_BANDS: usize,
        const PRESET_BANDS: usize,
        const MIN_VOLUME: i16,
        const MAX_VOLUME: i16,
        const FRACTION_DIGITS: u8,
    >(
        &mut self,
        database: Arc<OpenSCQ30Database>,
        device_model: DeviceModel,
        change_notify: watch::Sender<()>,
        state_modifier: Box<dyn StateModifier<T> + Send + Sync + 'static>,
        module_settings: EqualizerModuleSettings<
            VISIBLE_BANDS,
            PRESET_BANDS,
            MIN_VOLUME,
            MAX_VOLUME,
            FRACTION_DIGITS,
        >,
    ) where
        T: Has<EqualizerConfiguration<CHANNELS, BANDS, MIN_VOLUME, MAX_VOLUME, FRACTION_DIGITS>>
            + Clone
            + Send
            + Sync,
    {
        let profile_store = Arc::new(
            CustomEqualizerProfileStore::new(database, device_model, change_notify.to_owned())
                .await,
        );
        self.setting_manager.add_handler(
            CategoryId::Equalizer,
            EqualizerSettingHandler::<
                T,
                CHANNELS,
                BANDS,
                VISIBLE_BANDS,
                PRESET_BANDS,
                MIN_VOLUME,
                MAX_VOLUME,
                FRACTION_DIGITS,
            >::new(profile_store.to_owned(), module_settings),
        );
        self.setting_manager.add_handler(
            CategoryId::EqualizerImportExport,
            ImportExportSettingHandler::new(profile_store, change_notify),
        );
        self.state_modifiers.push(state_modifier);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use macaddr::MacAddr6;

    use crate::{
        api::settings::{Setting, SettingId},
        devices::soundcore::{a3876, a3952, d1101},
    };

    use super::*;

    const EXPECTED_COMMON_PRESETS: [(&str, u16, &str, [i16; 10]); 22] = [
        (
            "SoundcoreSignature",
            0x0000,
            "Soundcore Signature",
            [0, 0, 0, 0, 0, 0, 0, 0, 0, -120],
        ),
        (
            "Acoustic",
            0x0001,
            "Acoustic",
            [40, 10, 20, 20, 40, 40, 40, 20, 0, -120],
        ),
        (
            "BassBooster",
            0x0002,
            "Bass Booster",
            [40, 30, 10, 0, 0, 0, 0, 0, 0, -120],
        ),
        (
            "BassReducer",
            0x0003,
            "Bass Reducer",
            [-40, -30, -10, 0, 0, 0, 0, 0, 0, -120],
        ),
        (
            "Classical",
            0x0004,
            "Classical",
            [30, 30, -20, -20, 0, 20, 30, 40, 0, -120],
        ),
        (
            "Podcast",
            0x0005,
            "Podcast",
            [-30, 20, 40, 40, 30, 20, 0, -20, 0, -120],
        ),
        (
            "Dance",
            0x0006,
            "Dance",
            [20, -30, -10, 10, 20, 20, 10, -30, 0, -120],
        ),
        (
            "Deep",
            0x0007,
            "Deep",
            [20, 10, 30, 30, 20, -20, -40, -50, 0, -120],
        ),
        (
            "Electronic",
            0x0008,
            "Electronic",
            [30, 20, -20, 20, 10, 20, 30, 30, 0, -120],
        ),
        (
            "Flat",
            0x0009,
            "Flat",
            [-20, -20, -10, 0, 0, 0, -20, -20, 0, -120],
        ),
        (
            "HipHop",
            0x000a,
            "Hip Hop",
            [20, 30, -10, -10, 20, -10, 20, 30, 0, -120],
        ),
        (
            "Jazz",
            0x000b,
            "Jazz",
            [20, 20, -20, -20, 0, 20, 30, 40, 0, -120],
        ),
        (
            "Latin",
            0x000c,
            "Latin",
            [0, 0, -20, -20, -20, 0, 30, 50, 0, -120],
        ),
        (
            "Lounge",
            0x000d,
            "Lounge",
            [-10, 20, 40, 30, 0, -20, 20, 10, 0, -120],
        ),
        (
            "Piano",
            0x000e,
            "Piano",
            [0, 30, 30, 20, 40, 50, 30, 40, 0, -120],
        ),
        (
            "Pop",
            0x000f,
            "Pop",
            [-10, 10, 30, 30, 10, -10, -20, -30, 0, -120],
        ),
        (
            "RnB",
            0x0010,
            "RnB",
            [60, 20, -20, -20, 20, 30, 30, 40, 0, -120],
        ),
        (
            "Rock",
            0x0011,
            "Rock",
            [30, 20, -10, -10, 10, 30, 30, 30, 0, -120],
        ),
        (
            "SmallSpeakers",
            0x0012,
            "Small Speakers",
            [40, 30, 10, 0, -20, -30, -40, -40, 0, -120],
        ),
        (
            "SpokenWord",
            0x0013,
            "Spoken Word",
            [-30, -20, 10, 20, 20, 10, 0, -30, 0, -120],
        ),
        (
            "TrebleBooster",
            0x0014,
            "Treble Booster",
            [-20, -20, -20, -10, 10, 20, 20, 40, 0, -120],
        ),
        (
            "TrebleReducer",
            0x0015,
            "Treble Reducer",
            [0, 0, 0, -20, -30, -40, -40, -60, 0, -120],
        ),
    ];

    #[test]
    fn f2_common_settings_presets_intact() {
        let settings = common_settings();
        assert_eq!(settings.presets.len(), 22);
        for (actual, (name, id, localized, adjustments)) in
            settings.presets.iter().zip(EXPECTED_COMMON_PRESETS)
        {
            assert_eq!(actual.name, name);
            assert_eq!(actual.id, id);
            assert_eq!((actual.localized_name)(), localized);
            assert_eq!(actual.volume_adjustments.adjustments(), &adjustments);
        }
    }

    #[test]
    fn f2_common_settings_type_2_presets_intact() {
        let settings = common_settings_type_2();
        assert_eq!(settings.presets.len(), 22);
        for (actual, (name, id, localized, mut adjustments)) in
            settings.presets.iter().zip(EXPECTED_COMMON_PRESETS)
        {
            if name == "Rock" {
                adjustments = [30, 20, -10, -10, 10, 30, 40, 50, 0, -120];
            }
            assert_eq!(actual.name, name);
            assert_eq!(actual.id, id);
            assert_eq!((actual.localized_name)(), localized);
            assert_eq!(actual.volume_adjustments.adjustments(), &adjustments);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn f2_per_device_presets_intact() {
        let a3876_settings = a3876::equalizer_settings();
        assert_eq!(a3876_settings.presets.len(), 22);
        assert_eq!(a3876_settings.presets[1].name, "Balanced");
        assert_eq!(a3876_settings.presets[21].name, "VolumeBooster");
        assert_eq!(a3876_settings.presets[21].id, 30);

        let a3952_settings = a3952::equalizer_settings();
        assert_eq!(a3952_settings.presets.len(), 22);
        assert_eq!(
            a3952_settings.presets[1].volume_adjustments.adjustments(),
            &[30, -10, 0, 0, 20, 10, 10, -10, 0, -120]
        );

        let d1101_settings = d1101::equalizer_settings();
        assert_eq!(d1101_settings.presets.len(), 6);

        let database = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let expected_demo_counts = [
            (DeviceModel::SoundcoreA3005, 22 + 13),
            (DeviceModel::SoundcoreA3035, 22 + 13),
            (DeviceModel::SoundcoreA3040, 21 + 13),
            (DeviceModel::SoundcoreA3062, 22 + 13),
            (DeviceModel::SoundcoreD1402, 22 + 13),
            (DeviceModel::SoundcoreA3330, 1 + 13),
            (DeviceModel::SoundcoreD1101, 6 + 13),
            (DeviceModel::SoundcoreA3876, 22 + 13),
            (DeviceModel::SoundcoreA3952, 22 + 13),
            (DeviceModel::SoundcoreA3116, 6),
            (DeviceModel::SoundcoreA3909, 22),
            (DeviceModel::SoundcoreA3944, 22),
            (DeviceModel::SoundcoreA3949, 22),
        ];
        for (model, expected_len) in expected_demo_counts {
            let registry = model.demo_device_registry(database.clone()).await.unwrap();
            let device = registry.connect(MacAddr6::nil()).await.unwrap();
            let Some(Setting::PresetEqualizerProfileSelect { select, .. }) =
                device.setting(&SettingId::PresetEqualizerProfile)
            else {
                panic!("{model} missing PresetEqualizerProfileSelect");
            };
            assert_eq!(
                select.options.len(),
                expected_len,
                "{model} unexpected preset count"
            );
        }
    }

    #[test]
    fn f3_genre_presets_invariants() {
        let genre_presets = common_genre_presets();
        assert_eq!(genre_presets.len(), 13);

        let required = ["Trap", "Techno", "HardTechno"];
        for req in required {
            assert!(
                genre_presets.iter().any(|p| p.name == req),
                "missing required genre preset {req}"
            );
        }

        let mut seen_names = HashSet::new();
        let mut seen_lower_names = HashSet::new();
        let mut seen_visible_curves = HashSet::new();

        for base in [common_settings(), common_settings_type_2()] {
            assert_eq!(base.band_hz, COMMON_BAND_HZ);
            assert_eq!(base.preset_min_volume, -60);
            assert_eq!(base.preset_max_volume, 60);
            assert_eq!(base.custom_preset_id, Some(0xfefe));

            for preset in &base.presets {
                seen_visible_curves.insert(preset.volume_adjustments.adjustments()[..8].to_vec());
            }
            // Also reserve [0; 8] and [1; 8]
            seen_visible_curves.insert(vec![0; 8]);
            seen_visible_curves.insert(vec![1; 8]);
            // Also reserve a3028 test curve
            seen_visible_curves.insert(vec![-60, 60, 23, 40, 22, 60, -4, 16]);

            for preset in &base.presets {
                assert!(seen_names.insert(preset.name));
                assert!(seen_lower_names.insert(preset.name.to_ascii_lowercase()));
            }
            seen_names.clear();
            seen_lower_names.clear();
        }

        for base_preset in common_settings().presets {
            seen_names.insert(base_preset.name);
            seen_lower_names.insert(base_preset.name.to_ascii_lowercase());
        }
        assert!(seen_names.contains("HipHop"));
        assert!(seen_names.contains("Rock"));

        for preset in &genre_presets {
            assert_eq!(preset.id, 0xfefe);
            assert!(
                seen_names.insert(preset.name),
                "duplicate name {}",
                preset.name
            );
            assert!(
                seen_lower_names.insert(preset.name.to_ascii_lowercase()),
                "case-insensitive duplicate name {}",
                preset.name
            );
            let localized = (preset.localized_name)();
            assert!(!localized.is_empty(), "empty label for {}", preset.name);
            let adj = preset.volume_adjustments.adjustments();
            assert_eq!(&adj[8..10], &[0, -120]);
            for &band in &adj[..8] {
                assert!(
                    (-60..=60).contains(&band),
                    "band {band} out of -60..=60 for {}",
                    preset.name
                );
            }
            assert!(
                seen_visible_curves.insert(adj[..8].to_vec()),
                "duplicate or flat curve for {}",
                preset.name
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn f3_genre_presets_excluded_when_custom_preset_id_is_none() {
        let database = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        for model in [DeviceModel::SoundcoreA3944, DeviceModel::SoundcoreA3949] {
            let registry = model.demo_device_registry(database.clone()).await.unwrap();
            let device = registry.connect(MacAddr6::nil()).await.unwrap();
            let Some(Setting::PresetEqualizerProfileSelect { select, .. }) =
                device.setting(&SettingId::PresetEqualizerProfile)
            else {
                panic!("{model} missing PresetEqualizerProfileSelect");
            };
            assert_eq!(select.options.len(), 22);
            for genre in common_genre_presets() {
                assert!(!select.options.contains(&genre.name.into()));
            }
            assert!(device.setting(&SettingId::CustomEqualizerProfile).is_none());
            let Some(Setting::Equalizer { read_only, .. }) =
                device.setting(&SettingId::VolumeAdjustments)
            else {
                panic!("{model} missing VolumeAdjustments");
            };
            assert!(read_only);
        }

        let q30_registry = DeviceModel::SoundcoreA3028
            .demo_device_registry(database)
            .await
            .unwrap();
        let q30 = q30_registry.connect(MacAddr6::nil()).await.unwrap();
        let Some(Setting::PresetEqualizerProfileSelect {
            equalizer, select, ..
        }) = q30.setting(&SettingId::PresetEqualizerProfile)
        else {
            panic!("SoundcoreA3028 missing PresetEqualizerProfileSelect");
        };
        assert_eq!(
            equalizer.band_hz.as_ref(),
            &[100, 200, 400, 800, 1600, 3200, 6400, 12800]
        );
        assert_eq!(equalizer.min, -60);
        assert_eq!(equalizer.max, 60);
        for expected in ["Trap", "Techno", "HardTechno", "HipHop", "Rock"] {
            assert!(
                select.options.contains(&expected.into()),
                "SoundcoreA3028 missing {expected}"
            );
        }
    }
}
