use std::{borrow::Cow, collections::VecDeque, time::Duration};

use bitflags::bitflags;
use openscq30_i18n_macros::Translate;
use strum::{Display, EnumIter, EnumString, IntoStaticStr};
use tokio::time::Instant;

use super::aap::{AapInbound, BatteryEntry};

pub const SUPPORTED_APP2_MODEL_NUMBERS: &[&str] =
    &["A2931", "A2699", "A2698", "A3047", "A3048", "A3049"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryBud {
    Left,
    Right,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, IntoStaticStr, EnumIter, Translate,
)]
pub enum EarStatus {
    InEar,
    OutOfEar,
    InCase,
    Disconnected,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EarStatusRaw {
    InEar,
    OutOfEar,
    InCase,
    Disconnected,
    InMotion,
    Unknown,
}

impl EarStatusRaw {
    pub const fn to_ear_status(self) -> Option<EarStatus> {
        match self {
            Self::InEar => Some(EarStatus::InEar),
            Self::OutOfEar => Some(EarStatus::OutOfEar),
            Self::InCase => Some(EarStatus::InCase),
            Self::Disconnected => Some(EarStatus::Disconnected),
            Self::InMotion => None,
            Self::Unknown => Some(EarStatus::Unknown),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, IntoStaticStr, EnumIter, Translate,
)]
pub enum AapListeningMode {
    NoiseCanceling,
    Transparency,
    Adaptive,
    Off,
}

impl AapListeningMode {
    pub const fn wire_value(self) -> u8 {
        match self {
            Self::Off => 0x01,
            Self::NoiseCanceling => 0x02,
            Self::Transparency => 0x03,
            Self::Adaptive => 0x04,
        }
    }

    pub const fn from_wire_value(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::Off),
            0x02 => Some(Self::NoiseCanceling),
            0x03 => Some(Self::Transparency),
            0x04 => Some(Self::Adaptive),
            _ => None,
        }
    }

    pub const fn stem_bit(self) -> StemModeMask {
        match self {
            Self::Off => StemModeMask::OFF,
            Self::NoiseCanceling => StemModeMask::NOISE_CANCELING,
            Self::Transparency => StemModeMask::TRANSPARENCY,
            Self::Adaptive => StemModeMask::ADAPTIVE,
        }
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct StemModeMask: u8 {
        const OFF = 0x01;
        const NOISE_CANCELING = 0x02;
        const TRANSPARENCY = 0x04;
        const ADAPTIVE = 0x08;
    }
}

impl StemModeMask {
    pub fn to_listening_modes(self) -> Vec<AapListeningMode> {
        let mut modes = Vec::new();
        if self.contains(Self::NOISE_CANCELING) {
            modes.push(AapListeningMode::NoiseCanceling);
        }
        if self.contains(Self::TRANSPARENCY) {
            modes.push(AapListeningMode::Transparency);
        }
        if self.contains(Self::ADAPTIVE) {
            modes.push(AapListeningMode::Adaptive);
        }
        if self.contains(Self::OFF) {
            modes.push(AapListeningMode::Off);
        }
        modes
    }
}

pub fn sanitize_stem_modes(
    mask: StemModeMask,
    allow_off: bool,
    adaptive_capable: bool,
) -> StemModeMask {
    let mut sanitized = mask & StemModeMask::all();
    if !adaptive_capable {
        sanitized.remove(StemModeMask::ADAPTIVE);
    }
    if !allow_off {
        sanitized.remove(StemModeMask::OFF);
    }
    if sanitized.bits().count_ones() < 2 {
        sanitized.insert(StemModeMask::TRANSPARENCY);
    }
    if sanitized.bits().count_ones() < 2 {
        sanitized.insert(StemModeMask::NOISE_CANCELING);
    }
    sanitized
}

pub fn validate_user_stem_modes(
    selected: &[Cow<'static, str>],
    allow_off: bool,
    adaptive_capable: bool,
) -> Result<StemModeMask, String> {
    let mut mask = StemModeMask::empty();
    for item in selected {
        let mode: AapListeningMode = item
            .as_ref()
            .parse()
            .map_err(|_| format!("invalid stem cycle mode: {item}"))?;
        if mode == AapListeningMode::Off && !allow_off {
            return Err(
                "Off mode cannot be included when Allow Off Listening Mode is disabled".to_owned(),
            );
        }
        if mode == AapListeningMode::Adaptive && !adaptive_capable {
            return Err("Adaptive mode is not supported by this device".to_owned());
        }
        mask.insert(mode.stem_bit());
    }
    if mask.bits().count_ones() < 2 {
        return Err("at least two stem cycle modes must be selected".to_owned());
    }
    Ok(mask)
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, IntoStaticStr, EnumIter, Translate,
)]
pub enum MicSide {
    Automatic,
    Right,
    Left,
}

impl MicSide {
    pub const fn wire_value(self) -> u8 {
        match self {
            Self::Automatic => 0x00,
            Self::Right => 0x01,
            Self::Left => 0x02,
        }
    }

    pub const fn from_wire_value(v: u8) -> Option<Self> {
        match v {
            0x00 => Some(Self::Automatic),
            0x01 => Some(Self::Right),
            0x02 => Some(Self::Left),
            _ => None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, IntoStaticStr, EnumIter, Translate,
)]
pub enum PressSpeed {
    Default,
    Slower,
    Slowest,
}

impl PressSpeed {
    pub const fn wire_value(self) -> u8 {
        match self {
            Self::Default => 0x00,
            Self::Slower => 0x01,
            Self::Slowest => 0x02,
        }
    }

    pub const fn from_wire_value(v: u8) -> Option<Self> {
        match v {
            0x00 => Some(Self::Default),
            0x01 => Some(Self::Slower),
            0x02 => Some(Self::Slowest),
            _ => None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, IntoStaticStr, EnumIter, Translate,
)]
pub enum HoldDuration {
    Default,
    Shorter,
    Shortest,
}

impl HoldDuration {
    pub const fn wire_value(self) -> u8 {
        match self {
            Self::Default => 0x00,
            Self::Shorter => 0x01,
            Self::Shortest => 0x02,
        }
    }

    pub const fn from_wire_value(v: u8) -> Option<Self> {
        match v {
            0x00 => Some(Self::Default),
            0x01 => Some(Self::Shorter),
            0x02 => Some(Self::Shortest),
            _ => None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, IntoStaticStr, EnumIter, Translate,
)]
pub enum ConversationAwarenessState {
    Inactive,
    Speaking,
    Recovering,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryComponent {
    Headphone,
    Right,
    Left,
    Case,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryChargeStatus {
    Charging,
    Discharging,
    Disconnected,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ComponentBattery {
    pub percent: Option<u8>,
    pub charging: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceMetadata {
    pub name: Option<String>,
    pub model_number: Option<String>,
    pub manufacturer: Option<String>,
    pub serial_number: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProximityKeys {
    pub irk: Option<[u8; 16]>,
    pub enc_key: Option<[u8; 16]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct AirPodsPro2Capabilities {
    pub ear_detection: bool,
    pub mic_side: bool,
    pub press_speed: bool,
    pub hold_duration: bool,
    pub tone_volume: bool,
    pub personalized_volume: bool,
    pub listening_mode: bool,
    pub adaptive: bool,
    pub conversation_awareness: bool,
    pub one_bud_anc: bool,
    pub allow_off: bool,
    pub hearing_aid: bool,
    pub media_assist: bool,
}

impl Default for AirPodsPro2Capabilities {
    fn default() -> Self {
        Self {
            ear_detection: true,
            mic_side: true,
            press_speed: true,
            hold_duration: true,
            tone_volume: true,
            personalized_volume: true,
            listening_mode: true,
            adaptive: true,
            conversation_awareness: true,
            one_bud_anc: true,
            allow_off: true,
            hearing_aid: true,
            media_assist: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    Connecting,
    HandshakeSent,
    Ready,
    InitialReportDone,
    AttachGaveUp,
    Disconnected,
}

impl SessionPhase {
    pub const fn is_initial_report_phase(self) -> bool {
        matches!(self, Self::Connecting | Self::HandshakeSent | Self::Ready)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PersistedHostSettings {
    pub allow_off: Option<bool>,
    pub stem_modes: Option<StemModeMask>,
    pub ear_detection: Option<bool>,
    pub mic_side: Option<MicSide>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingListeningMode {
    pub mode: AapListeningMode,
    pub sent_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct AirPodsPro2State {
    pub phase: SessionPhase,
    pub capabilities: AirPodsPro2Capabilities,
    pub metadata: DeviceMetadata,
    pub is_recognized_app2_model: Option<bool>,
    pub battery_left: ComponentBattery,
    pub battery_right: ComponentBattery,
    pub battery_case: ComponentBattery,
    pub primary_bud: Option<PrimaryBud>,
    pub primary_bud_authoritative: bool,
    pub raw_primary_ear: Option<EarStatus>,
    pub raw_secondary_ear: Option<EarStatus>,
    pub latest_device_listening_mode: Option<AapListeningMode>,
    pub optimistic_listening_modes: VecDeque<PendingListeningMode>,
    pub allow_off: bool,
    pub one_bud_anc: Option<bool>,
    pub adaptive_strength: Option<u8>,
    pub ear_detection: bool,
    pub mic_side: MicSide,
    pub stem_modes: StemModeMask,
    pub press_speed: PressSpeed,
    pub hold_duration: HoldDuration,
    pub tone_volume: u8,
    pub personalized_volume: bool,
    pub conversation_awareness: bool,
    pub ca_state: ConversationAwarenessState,
    pub hearing_aid_enrolled: Option<bool>,
    pub hearing_aid_enabled: bool,
    pub hearing_assist_enabled: bool,
    pub pme_record: Option<[u8; 140]>,
    pub media_assist_calls: bool,
    pub media_assist_media: bool,
    pub proximity_keys: ProximityKeys,
    pub call_audio_active: bool,
    pub sco_active: bool,
    pub saw_any_0x09: bool,
    pub persisted: PersistedHostSettings,
}

impl Default for AirPodsPro2State {
    fn default() -> Self {
        Self {
            phase: SessionPhase::Connecting,
            capabilities: AirPodsPro2Capabilities::default(),
            metadata: DeviceMetadata::default(),
            is_recognized_app2_model: None,
            battery_left: ComponentBattery::default(),
            battery_right: ComponentBattery::default(),
            battery_case: ComponentBattery::default(),
            primary_bud: None,
            primary_bud_authoritative: false,
            raw_primary_ear: None,
            raw_secondary_ear: None,
            latest_device_listening_mode: None,
            optimistic_listening_modes: VecDeque::new(),
            allow_off: true,
            one_bud_anc: None,
            adaptive_strength: None,
            ear_detection: true,
            mic_side: MicSide::Automatic,
            stem_modes: StemModeMask::NOISE_CANCELING
                | StemModeMask::TRANSPARENCY
                | StemModeMask::ADAPTIVE,
            press_speed: PressSpeed::Default,
            hold_duration: HoldDuration::Default,
            tone_volume: 100,
            personalized_volume: false,
            conversation_awareness: false,
            ca_state: ConversationAwarenessState::Inactive,
            hearing_aid_enrolled: None,
            hearing_aid_enabled: false,
            hearing_assist_enabled: false,
            pme_record: None,
            media_assist_calls: false,
            media_assist_media: false,
            proximity_keys: ProximityKeys::default(),
            call_audio_active: false,
            sco_active: false,
            saw_any_0x09: false,
            persisted: PersistedHostSettings::default(),
        }
    }
}

impl AirPodsPro2State {
    pub fn left_ear(&self) -> Option<EarStatus> {
        match self.primary_bud? {
            PrimaryBud::Left => self.raw_primary_ear,
            PrimaryBud::Right => self.raw_secondary_ear,
        }
    }

    pub fn right_ear(&self) -> Option<EarStatus> {
        match self.primary_bud? {
            PrimaryBud::Left => self.raw_secondary_ear,
            PrimaryBud::Right => self.raw_primary_ear,
        }
    }

    pub fn available_listening_modes(&self) -> Vec<AapListeningMode> {
        let mut modes = vec![
            AapListeningMode::NoiseCanceling,
            AapListeningMode::Transparency,
        ];
        if self.capabilities.adaptive {
            modes.push(AapListeningMode::Adaptive);
        }
        if self.allow_off {
            modes.push(AapListeningMode::Off);
        }
        modes
    }

    pub fn restore_persisted_host_settings(&mut self) {
        if let Some(allow_off) = self.persisted.allow_off {
            self.allow_off = allow_off;
        }
        if let Some(stem_modes) = self.persisted.stem_modes {
            self.stem_modes =
                sanitize_stem_modes(stem_modes, self.allow_off, self.capabilities.adaptive);
        } else {
            self.stem_modes =
                sanitize_stem_modes(self.stem_modes, self.allow_off, self.capabilities.adaptive);
        }
        if let Some(ear_detection) = self.persisted.ear_detection {
            self.ear_detection = ear_detection;
        }
        if let Some(mic_side) = self.persisted.mic_side {
            self.mic_side = mic_side;
        }
    }

    pub fn unclamped_listening_mode(
        &self,
        now: Instant,
        quiet_window: Duration,
    ) -> AapListeningMode {
        if let Some(pending) = self
            .optimistic_listening_modes
            .iter()
            .rev()
            .find(|pending| now.saturating_duration_since(pending.sent_at) < quiet_window)
        {
            pending.mode
        } else {
            self.latest_device_listening_mode
                .unwrap_or(AapListeningMode::Adaptive)
        }
    }

    const fn clamp_listening_mode(&self, mode: AapListeningMode) -> AapListeningMode {
        if !self.allow_off && matches!(mode, AapListeningMode::Off) {
            AapListeningMode::Transparency
        } else {
            mode
        }
    }

    pub fn effective_listening_mode(
        &self,
        now: Instant,
        quiet_window: Duration,
    ) -> AapListeningMode {
        self.clamp_listening_mode(self.unclamped_listening_mode(now, quiet_window))
    }

    pub fn record_optimistic_listening_mode(&mut self, mode: AapListeningMode, now: Instant) {
        self.optimistic_listening_modes
            .push_back(PendingListeningMode { mode, sent_at: now });
    }

    pub fn rollback_last_optimistic_listening_mode(&mut self) {
        self.optimistic_listening_modes.pop_back();
    }

    pub fn prune_expired_optimistic_modes(&mut self, now: Instant, quiet_window: Duration) -> bool {
        let Some(last_pending) = self.optimistic_listening_modes.back().copied() else {
            return false;
        };
        let mut popped = false;
        while let Some(front) = self.optimistic_listening_modes.front() {
            if now.saturating_duration_since(front.sent_at) >= quiet_window {
                self.optimistic_listening_modes.pop_front();
                popped = true;
            } else {
                break;
            }
        }
        if !popped || quiet_window.is_zero() {
            return false;
        }
        let before = self.clamp_listening_mode(last_pending.mode);
        let after = self.effective_listening_mode(now, quiet_window);
        before != after
    }

    pub fn next_optimistic_expiry(&self, quiet_window: Duration) -> Option<Instant> {
        self.optimistic_listening_modes
            .front()
            .map(|front| front.sent_at + quiet_window)
    }

    pub const fn hearing_assist_active(&self) -> bool {
        self.hearing_aid_enabled || self.media_assist_calls || self.media_assist_media
    }

    pub fn apply_inbound(&mut self, inbound: AapInbound, now: Instant, quiet_window: Duration) {
        match inbound {
            AapInbound::DeviceInfo(info) => {
                self.is_recognized_app2_model = info
                    .model_number
                    .as_deref()
                    .map(|m| SUPPORTED_APP2_MODEL_NUMBERS.contains(&m));
                self.metadata = info;
            }
            AapInbound::Battery(entries) => {
                self.apply_battery_entries(&entries);
            }
            AapInbound::EarDetection { primary, secondary } => {
                if let Some(status) = primary.to_ear_status() {
                    self.raw_primary_ear = Some(status);
                }
                if let Some(status) = secondary.to_ear_status() {
                    self.raw_secondary_ear = Some(status);
                }
            }
            AapInbound::BudRole(role) => {
                self.primary_bud = Some(role);
                self.primary_bud_authoritative = true;
            }
            AapInbound::CallStatus {
                call_audio_active,
                sco_active,
            } => {
                self.call_audio_active = call_audio_active;
                self.sco_active = sco_active;
            }
            AapInbound::ProximityKeys { irk, enc_key } => {
                if irk.is_some() {
                    self.proximity_keys.irk = irk;
                }
                if enc_key.is_some() {
                    self.proximity_keys.enc_key = enc_key;
                }
            }
            AapInbound::CaSpeakingLevel(level) => match level {
                1..=2 => self.ca_state = ConversationAwarenessState::Speaking,
                3..=7 => self.ca_state = ConversationAwarenessState::Recovering,
                8..=9 => self.ca_state = ConversationAwarenessState::Inactive,
                _ => {}
            },
            AapInbound::PmeRecord {
                raw,
                enrolled,
                calls,
                media,
            } => {
                self.pme_record = Some(*raw);
                self.hearing_aid_enrolled = Some(enrolled);
                self.media_assist_calls = calls;
                self.media_assist_media = media;
            }
            AapInbound::Setting(setting) => {
                self.saw_any_0x09 = true;
                let protect_persisted = self.phase.is_initial_report_phase();
                match setting {
                    super::aap::AapSettingUpdate::MicSide(side) => {
                        if !(protect_persisted && self.persisted.mic_side.is_some()) {
                            self.mic_side = side;
                        }
                    }
                    super::aap::AapSettingUpdate::EarDetection(enabled) => {
                        if !(protect_persisted && self.persisted.ear_detection.is_some()) {
                            self.ear_detection = enabled;
                        }
                    }
                    super::aap::AapSettingUpdate::ListeningMode(dev_mode) => {
                        self.latest_device_listening_mode = Some(dev_mode);
                        let _ = self.prune_expired_optimistic_modes(now, quiet_window);
                        if let Some(pos) = self
                            .optimistic_listening_modes
                            .iter()
                            .position(|p| p.mode == dev_mode)
                        {
                            self.optimistic_listening_modes.drain(0..=pos);
                        }
                    }
                    super::aap::AapSettingUpdate::PressSpeed(speed) => self.press_speed = speed,
                    super::aap::AapSettingUpdate::HoldDuration(dur) => self.hold_duration = dur,
                    super::aap::AapSettingUpdate::StemModes(mask) => {
                        if !(protect_persisted && self.persisted.stem_modes.is_some()) {
                            self.stem_modes = sanitize_stem_modes(
                                mask,
                                self.allow_off,
                                self.capabilities.adaptive,
                            );
                        }
                    }
                    super::aap::AapSettingUpdate::OneBudAnc(enabled) => {
                        self.one_bud_anc = Some(enabled);
                    }
                    super::aap::AapSettingUpdate::ToneVolume(vol) => self.tone_volume = vol,
                    super::aap::AapSettingUpdate::PersonalizedVolume(enabled) => {
                        self.personalized_volume = enabled;
                    }
                    super::aap::AapSettingUpdate::ConversationAwareness(enabled) => {
                        self.conversation_awareness = enabled;
                        if !enabled {
                            self.ca_state = ConversationAwarenessState::Inactive;
                        }
                    }
                    super::aap::AapSettingUpdate::HearingAid(enabled) => {
                        self.hearing_aid_enabled = enabled;
                    }
                    super::aap::AapSettingUpdate::AdaptiveStrength(strength) => {
                        self.capabilities.adaptive = true;
                        self.adaptive_strength = Some(strength);
                    }
                    super::aap::AapSettingUpdate::HearingAssist(enabled) => {
                        self.hearing_assist_enabled = enabled;
                    }
                    super::aap::AapSettingUpdate::AllowOff(enabled) => {
                        if !(protect_persisted && self.persisted.allow_off.is_some()) {
                            self.allow_off = enabled;
                        }
                    }
                }
            }
            AapInbound::Unknown => {}
        }
    }

    fn apply_battery_entries(&mut self, entries: &[BatteryEntry]) {
        if !self.primary_bud_authoritative
            && let Some(first_pod) = entries.iter().find_map(|entry| match entry.component {
                BatteryComponent::Left => Some(PrimaryBud::Left),
                BatteryComponent::Right => Some(PrimaryBud::Right),
                _ => None,
            })
        {
            self.primary_bud = Some(first_pod);
        }

        for entry in entries {
            let battery = ComponentBattery {
                percent: entry.percent,
                charging: entry.status == BatteryChargeStatus::Charging,
            };
            match entry.component {
                BatteryComponent::Left => self.battery_left = battery,
                BatteryComponent::Right => self.battery_right = battery,
                BatteryComponent::Case => self.battery_case = battery,
                BatteryComponent::Headphone | BatteryComponent::Unknown => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_listening_mode_clamps_off_to_transparency_when_allow_off_disabled() {
        let now = Instant::now();
        let quiet_window = Duration::from_millis(3000);
        let mut state = AirPodsPro2State {
            allow_off: false,
            latest_device_listening_mode: Some(AapListeningMode::Off),
            ..Default::default()
        };

        assert!(
            !state
                .available_listening_modes()
                .contains(&AapListeningMode::Off)
        );
        assert_eq!(
            state.effective_listening_mode(now, quiet_window),
            AapListeningMode::Transparency
        );

        // Even if an optimistic Off entry was recorded before allow_off became false, it must clamp
        state.record_optimistic_listening_mode(AapListeningMode::Off, now);
        assert_eq!(
            state.effective_listening_mode(now, quiet_window),
            AapListeningMode::Transparency
        );

        // When allow_off is re-enabled, Off is allowed and returned
        state.allow_off = true;
        assert_eq!(
            state.effective_listening_mode(now, quiet_window),
            AapListeningMode::Off
        );
    }

    #[test]
    fn unclamped_listening_mode_returns_off_when_allow_off_disabled() {
        let now = Instant::now();
        let quiet_window = Duration::from_millis(3000);
        let mut state = AirPodsPro2State {
            allow_off: false,
            latest_device_listening_mode: Some(AapListeningMode::Off),
            ..Default::default()
        };

        // Confirmed latest_device_listening_mode == Some(Off)
        assert_eq!(
            state.unclamped_listening_mode(now, quiet_window),
            AapListeningMode::Off
        );
        assert_eq!(
            state.effective_listening_mode(now, quiet_window),
            AapListeningMode::Transparency
        );

        // Optimistic Off overrides a different confirmed mode, and remains unclamped while effective clamps
        state.latest_device_listening_mode = Some(AapListeningMode::Adaptive);
        state.record_optimistic_listening_mode(AapListeningMode::Off, now);
        assert_eq!(
            state.unclamped_listening_mode(now, quiet_window),
            AapListeningMode::Off
        );
        assert_eq!(
            state.effective_listening_mode(now, quiet_window),
            AapListeningMode::Transparency
        );
    }

    #[test]
    fn prune_expired_optimistic_modes_reports_change_when_expiry_reverts_mode() {
        let t0 = Instant::now();
        let quiet_window = Duration::from_millis(3000);
        let mut state = AirPodsPro2State {
            allow_off: true,
            latest_device_listening_mode: Some(AapListeningMode::NoiseCanceling),
            ..Default::default()
        };

        state.record_optimistic_listening_mode(AapListeningMode::Adaptive, t0);
        assert!(
            !state.prune_expired_optimistic_modes(t0 + Duration::from_millis(2999), quiet_window)
        );
        assert_eq!(state.optimistic_listening_modes.len(), 1);

        assert!(state.prune_expired_optimistic_modes(t0 + quiet_window, quiet_window));
        assert!(state.optimistic_listening_modes.is_empty());
        assert_eq!(
            state.effective_listening_mode(t0 + quiet_window, quiet_window),
            AapListeningMode::NoiseCanceling
        );
    }
}
