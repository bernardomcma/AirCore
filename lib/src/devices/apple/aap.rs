use thiserror::Error;

use super::state::{
    AapListeningMode, BatteryChargeStatus, BatteryComponent, DeviceMetadata, EarStatusRaw,
    HoldDuration, MicSide, PressSpeed, PrimaryBud, StemModeMask,
};

pub const AAP_HEADER: [u8; 4] = [0x04, 0x00, 0x04, 0x00];
pub const HANDSHAKE_HEADER: [u8; 4] = [0x00, 0x00, 0x04, 0x00];
pub const PME_RECORD_LEN: usize = 140;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum AapEncodeError {
    #[error("tone volume {0} out of valid range 0..=100")]
    InvalidToneVolume(u8),
    #[error("invalid 140-byte PME record header")]
    InvalidPmeRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatteryEntry {
    pub component: BatteryComponent,
    pub percent: Option<u8>,
    pub status: BatteryChargeStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AapSettingUpdate {
    MicSide(MicSide),
    EarDetection(bool),
    ListeningMode(AapListeningMode),
    PressSpeed(PressSpeed),
    HoldDuration(HoldDuration),
    StemModes(StemModeMask),
    OneBudAnc(bool),
    ToneVolume(u8),
    PersonalizedVolume(bool),
    ConversationAwareness(bool),
    HearingAid(bool),
    AdaptiveStrength(u8),
    HearingAssist(bool),
    AllowOff(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AapInbound {
    DeviceInfo(DeviceMetadata),
    Battery(Vec<BatteryEntry>),
    EarDetection {
        primary: EarStatusRaw,
        secondary: EarStatusRaw,
    },
    BudRole(PrimaryBud),
    CallStatus {
        call_audio_active: bool,
        sco_active: bool,
    },
    ProximityKeys {
        irk: Option<[u8; 16]>,
        enc_key: Option<[u8; 16]>,
    },
    CaSpeakingLevel(u8),
    PmeRecord {
        raw: Box<[u8; PME_RECORD_LEN]>,
        enrolled: bool,
        calls: bool,
        media: bool,
    },
    Setting(AapSettingUpdate),
    Unknown,
}

pub fn handshake() -> Vec<u8> {
    vec![
        0x00, 0x00, 0x04, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ]
}

pub fn feature_flags() -> Vec<u8> {
    vec![
        0x04, 0x00, 0x04, 0x00, 0x4D, 0x00, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]
}

pub fn request_notifications(ear_detection: bool) -> Vec<u8> {
    let ear_byte = if ear_detection { 0xFF } else { 0xFD };
    vec![
        0x04, 0x00, 0x04, 0x00, 0x0F, 0x00, 0xFF, 0xFF, ear_byte, 0xFF,
    ]
}

pub fn request_proximity_keys() -> Vec<u8> {
    vec![0x04, 0x00, 0x04, 0x00, 0x30, 0x00, 0x05, 0x00]
}

fn encode_setting_byte(sub_opcode: u8, val: u8) -> Vec<u8> {
    vec![
        0x04, 0x00, 0x04, 0x00, 0x09, 0x00, sub_opcode, val, 0x00, 0x00, 0x00,
    ]
}

const fn bool_byte(enabled: bool) -> u8 {
    if enabled { 0x01 } else { 0x02 }
}

pub fn set_mic_side(side: MicSide) -> Vec<u8> {
    encode_setting_byte(0x01, side.wire_value())
}

pub fn set_ear_detection(enabled: bool) -> Vec<u8> {
    encode_setting_byte(0x0A, bool_byte(enabled))
}

pub fn set_listening_mode(mode: AapListeningMode) -> Vec<u8> {
    encode_setting_byte(0x0D, mode.wire_value())
}

pub fn set_press_speed(speed: PressSpeed) -> Vec<u8> {
    encode_setting_byte(0x17, speed.wire_value())
}

pub fn set_hold_duration(duration: HoldDuration) -> Vec<u8> {
    encode_setting_byte(0x18, duration.wire_value())
}

pub fn set_stem_modes(mask: StemModeMask) -> Vec<u8> {
    encode_setting_byte(0x1A, mask.bits() & 0x0F)
}

pub fn set_one_bud_anc(enabled: bool) -> Vec<u8> {
    encode_setting_byte(0x1B, bool_byte(enabled))
}

pub fn set_tone_volume(volume: u8) -> Result<Vec<u8>, AapEncodeError> {
    if volume > 100 {
        return Err(AapEncodeError::InvalidToneVolume(volume));
    }
    Ok(vec![
        0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x1F, volume, 0x50, 0x00, 0x00,
    ])
}

pub fn set_personalized_volume(enabled: bool) -> Vec<u8> {
    encode_setting_byte(0x26, bool_byte(enabled))
}

pub fn set_conversation_awareness(enabled: bool) -> Vec<u8> {
    encode_setting_byte(0x28, bool_byte(enabled))
}

pub fn set_hearing_aid(enabled: bool) -> Vec<u8> {
    vec![
        0x04,
        0x00,
        0x04,
        0x00,
        0x09,
        0x00,
        0x2C,
        0x01,
        bool_byte(enabled),
        0x00,
        0x00,
    ]
}

pub fn set_hearing_assist(enabled: bool) -> Vec<u8> {
    encode_setting_byte(0x33, bool_byte(enabled))
}

pub fn set_allow_off(enabled: bool) -> Vec<u8> {
    encode_setting_byte(0x34, bool_byte(enabled))
}

pub fn encode_pme_record(
    record: &[u8; PME_RECORD_LEN],
    calls: bool,
    media: bool,
) -> Result<[u8; PME_RECORD_LEN], AapEncodeError> {
    if record[0..9] != [0x04, 0x00, 0x04, 0x00, 0x53, 0x00, 0x84, 0x00, 0x02] {
        return Err(AapEncodeError::InvalidPmeRecord);
    }
    let mut out = *record;
    out[10] = bool_byte(calls);
    out[11] = bool_byte(media);
    Ok(out)
}

pub fn parse_inbound(pkt: &[u8]) -> Option<AapInbound> {
    if pkt.len() < 6 {
        return None;
    }
    if pkt[0..4] != AAP_HEADER {
        return if pkt[0..4] == HANDSHAKE_HEADER || pkt[0..4] == [0x01, 0x00, 0x04, 0x00] {
            Some(AapInbound::Unknown)
        } else {
            None
        };
    }

    let opcode = u16::from_le_bytes([pkt[4], pkt[5]]);
    match opcode {
        0x001D => parse_device_info(pkt),
        0x0004 => parse_battery(pkt),
        0x0006 => parse_ear_detection(pkt),
        0x0008 => parse_bud_role(pkt),
        0x000E => parse_call_status(pkt),
        0x0031 => parse_proximity_keys(pkt),
        0x004B => parse_ca_speaking_level(pkt),
        0x0053 => parse_pme_record(pkt),
        0x0009 => parse_setting(pkt),
        _ => Some(AapInbound::Unknown),
    }
}

fn parse_device_info(pkt: &[u8]) -> Option<AapInbound> {
    if pkt.len() < 12 {
        return None;
    }
    let strings: Vec<Option<String>> = pkt[11..]
        .split(|&b| b == 0)
        .map(|slice| {
            let s = String::from_utf8_lossy(slice).trim().to_owned();
            if s.is_empty() { None } else { Some(s) }
        })
        .collect();

    Some(AapInbound::DeviceInfo(DeviceMetadata {
        name: strings.first().cloned().flatten(),
        model_number: strings.get(1).cloned().flatten(),
        manufacturer: strings.get(2).cloned().flatten(),
        serial_number: strings.get(3).cloned().flatten(),
    }))
}

fn parse_battery(pkt: &[u8]) -> Option<AapInbound> {
    if pkt.len() < 7 {
        return None;
    }
    let count = usize::from(pkt[6]);
    if pkt.len() < 7 + count * 5 {
        return None;
    }
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let base = 7 + i * 5;
        let component = match pkt[base] {
            0x01 => BatteryComponent::Headphone,
            0x02 => BatteryComponent::Right,
            0x04 => BatteryComponent::Left,
            0x08 => BatteryComponent::Case,
            _ => BatteryComponent::Unknown,
        };
        let raw_level = pkt[base + 2];
        let status = match pkt[base + 3] {
            0x01 | 0x05 => BatteryChargeStatus::Charging,
            0x02 => BatteryChargeStatus::Discharging,
            0x04 => BatteryChargeStatus::Disconnected,
            _ => BatteryChargeStatus::Unknown,
        };
        let percent = if status == BatteryChargeStatus::Disconnected || raw_level == 0xFF {
            None
        } else {
            Some(raw_level.min(100))
        };
        entries.push(BatteryEntry {
            component,
            percent,
            status,
        });
    }
    Some(AapInbound::Battery(entries))
}

const fn parse_ear_byte(b: u8) -> EarStatusRaw {
    match b {
        0x00 => EarStatusRaw::InEar,
        0x01 => EarStatusRaw::OutOfEar,
        0x02 => EarStatusRaw::InCase,
        0x03 => EarStatusRaw::Disconnected,
        0x04 => EarStatusRaw::InMotion,
        _ => EarStatusRaw::Unknown,
    }
}

fn parse_ear_detection(pkt: &[u8]) -> Option<AapInbound> {
    if pkt.len() < 8 {
        return None;
    }
    Some(AapInbound::EarDetection {
        primary: parse_ear_byte(pkt[6]),
        secondary: parse_ear_byte(pkt[7]),
    })
}

fn parse_bud_role(pkt: &[u8]) -> Option<AapInbound> {
    if pkt.len() < 7 {
        return None;
    }
    match pkt[6] {
        0x01 => Some(AapInbound::BudRole(PrimaryBud::Left)),
        0x02 => Some(AapInbound::BudRole(PrimaryBud::Right)),
        _ => None,
    }
}

fn parse_call_status(pkt: &[u8]) -> Option<AapInbound> {
    if pkt.len() <= 12 {
        return Some(AapInbound::Unknown);
    }
    Some(AapInbound::CallStatus {
        call_audio_active: (pkt[12] & 0x01) != 0,
        sco_active: (pkt[12] & 0x20) != 0,
    })
}

fn parse_proximity_keys(pkt: &[u8]) -> Option<AapInbound> {
    if pkt.len() < 7 {
        return None;
    }
    let count = usize::from(pkt[6]);
    let mut offset = 7;
    let mut irk = None;
    let mut enc_key = None;

    for _ in 0..count {
        if offset + 4 > pkt.len() {
            break;
        }
        let key_type = u16::from_le_bytes([pkt[offset], pkt[offset + 1]]);
        let key_len = usize::from(u16::from_le_bytes([pkt[offset + 2], pkt[offset + 3]]));
        offset += 4;
        if offset + key_len > pkt.len() {
            break;
        }
        if key_len == 16 {
            let mut key = [0_u8; 16];
            key.copy_from_slice(&pkt[offset..offset + 16]);
            match key_type {
                0x0001 => irk = Some(key),
                0x0004 => enc_key = Some(key),
                _ => {}
            }
        }
        offset += key_len;
    }

    Some(AapInbound::ProximityKeys { irk, enc_key })
}

fn parse_ca_speaking_level(pkt: &[u8]) -> Option<AapInbound> {
    if pkt.len() < 10 || pkt[6..9] != [0x02, 0x00, 0x01] {
        return None;
    }
    Some(AapInbound::CaSpeakingLevel(pkt[9]))
}

fn parse_pme_record(pkt: &[u8]) -> Option<AapInbound> {
    if pkt.len() != PME_RECORD_LEN || pkt[6..9] != [0x84, 0x00, 0x02] {
        return None;
    }
    let mut raw = Box::new([0_u8; PME_RECORD_LEN]);
    raw.copy_from_slice(pkt);
    Some(AapInbound::PmeRecord {
        enrolled: pkt[9] == 0x01,
        calls: pkt[10] == 0x01,
        media: pkt[11] == 0x01,
        raw,
    })
}

const fn decode_bool_byte(v: u8) -> Option<bool> {
    match v {
        0x01 => Some(true),
        0x02 => Some(false),
        _ => None,
    }
}

fn parse_setting(pkt: &[u8]) -> Option<AapInbound> {
    if pkt.len() < 8 {
        return None;
    }
    let sub = pkt[6];
    let val = pkt[7];
    let update = match sub {
        0x01 => AapSettingUpdate::MicSide(MicSide::from_wire_value(val)?),
        0x0A => AapSettingUpdate::EarDetection(decode_bool_byte(val)?),
        0x0D => AapSettingUpdate::ListeningMode(AapListeningMode::from_wire_value(val)?),
        0x17 => AapSettingUpdate::PressSpeed(PressSpeed::from_wire_value(val)?),
        0x18 => AapSettingUpdate::HoldDuration(HoldDuration::from_wire_value(val)?),
        0x1A => AapSettingUpdate::StemModes(StemModeMask::from_bits_truncate(val & 0x0F)),
        0x1B => AapSettingUpdate::OneBudAnc(decode_bool_byte(val)?),
        0x1F => AapSettingUpdate::ToneVolume(val.min(100)),
        0x26 => AapSettingUpdate::PersonalizedVolume(decode_bool_byte(val)?),
        0x28 => AapSettingUpdate::ConversationAwareness(decode_bool_byte(val)?),
        0x2C => {
            if pkt.len() < 9 || val != 0x01 {
                return None;
            }
            AapSettingUpdate::HearingAid(decode_bool_byte(pkt[8])?)
        }
        0x2E => AapSettingUpdate::AdaptiveStrength(val.min(100)),
        0x33 => AapSettingUpdate::HearingAssist(decode_bool_byte(val)?),
        0x34 => AapSettingUpdate::AllowOff(decode_bool_byte(val)?),
        _ => return Some(AapInbound::Unknown),
    };
    Some(AapInbound::Setting(update))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_handshake_and_init_packets() {
        assert_eq!(
            handshake(),
            vec![
                0x00, 0x00, 0x04, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00
            ]
        );
        assert_eq!(
            feature_flags(),
            vec![
                0x04, 0x00, 0x04, 0x00, 0x4D, 0x00, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00
            ]
        );
        assert_eq!(
            request_notifications(true),
            vec![0x04, 0x00, 0x04, 0x00, 0x0F, 0x00, 0xFF, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(
            request_notifications(false),
            vec![0x04, 0x00, 0x04, 0x00, 0x0F, 0x00, 0xFF, 0xFF, 0xFD, 0xFF]
        );
        assert_eq!(
            request_proximity_keys(),
            vec![0x04, 0x00, 0x04, 0x00, 0x30, 0x00, 0x05, 0x00]
        );
    }

    #[test]
    fn encodes_tone_volume_with_0x50_extra_byte_and_hearing_aid_0x2c() {
        let pkt = set_tone_volume(80).unwrap();
        assert_eq!(
            pkt,
            vec![
                0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x1F, 80, 0x50, 0x00, 0x00
            ]
        );
        assert_eq!(
            set_tone_volume(101),
            Err(AapEncodeError::InvalidToneVolume(101))
        );

        assert_eq!(
            set_hearing_aid(true),
            vec![
                0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x2C, 0x01, 0x01, 0x00, 0x00
            ]
        );
        assert_eq!(
            set_hearing_aid(false),
            vec![
                0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x2C, 0x01, 0x02, 0x00, 0x00
            ]
        );
    }

    #[test]
    fn parses_device_info_0x1d() {
        let mut pkt = vec![
            0x04, 0x00, 0x04, 0x00, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        pkt.extend_from_slice(b"My AirPods\0A3048\0Apple Inc.\0SERIAL123\0");
        let parsed = parse_inbound(&pkt).unwrap();
        assert_eq!(
            parsed,
            AapInbound::DeviceInfo(DeviceMetadata {
                name: Some("My AirPods".to_owned()),
                model_number: Some("A3048".to_owned()),
                manufacturer: Some("Apple Inc.".to_owned()),
                serial_number: Some("SERIAL123".to_owned()),
            })
        );
        assert!(parse_inbound(&[0x04, 0x00, 0x04, 0x00, 0x1D, 0x00, 0, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn parses_battery_0x04_edge_cases() {
        let pkt = vec![
            0x04, 0x00, 0x04, 0x00, 0x04, 0x00, 0x03, // count = 3
            0x04, 0x01, 120, 0x02, 0x01, // Left: clamped to 100, discharging
            0x02, 0x01, 0xFF, 0x04, 0x01, // Right: absent/disconnected
            0x08, 0x01, 75, 0x05, 0x01, // Case: optimized charging -> Charging
        ];
        assert_eq!(
            parse_inbound(&pkt).unwrap(),
            AapInbound::Battery(vec![
                BatteryEntry {
                    component: BatteryComponent::Left,
                    percent: Some(100),
                    status: BatteryChargeStatus::Discharging,
                },
                BatteryEntry {
                    component: BatteryComponent::Right,
                    percent: None,
                    status: BatteryChargeStatus::Disconnected,
                },
                BatteryEntry {
                    component: BatteryComponent::Case,
                    percent: Some(75),
                    status: BatteryChargeStatus::Charging,
                },
            ])
        );
        // Truncated packet rejected
        assert!(parse_inbound(&pkt[..pkt.len() - 1]).is_none());
    }

    #[test]
    fn parses_ear_detection_bud_role_ca_and_proximity_keys() {
        assert_eq!(
            parse_inbound(&[0x04, 0x00, 0x04, 0x00, 0x06, 0x00, 0x00, 0x04]).unwrap(),
            AapInbound::EarDetection {
                primary: EarStatusRaw::InEar,
                secondary: EarStatusRaw::InMotion,
            }
        );
        assert_eq!(
            parse_inbound(&[0x04, 0x00, 0x04, 0x00, 0x08, 0x00, 0x02]).unwrap(),
            AapInbound::BudRole(PrimaryBud::Right)
        );
        assert_eq!(
            parse_inbound(&[0x04, 0x00, 0x04, 0x00, 0x4B, 0x00, 0x02, 0x00, 0x01, 0x05]).unwrap(),
            AapInbound::CaSpeakingLevel(5)
        );

        let mut keys_pkt = vec![0x04, 0x00, 0x04, 0x00, 0x31, 0x00, 0x03];
        // 0x0001 IRK 16B
        keys_pkt.extend_from_slice(&[0x01, 0x00, 16, 0x00]);
        keys_pkt.extend_from_slice(&[0xAA; 16]);
        // 0x0099 non-16B ignored
        keys_pkt.extend_from_slice(&[0x99, 0x00, 4, 0x00, 1, 2, 3, 4]);
        // 0x0004 ENC_KEY 16B
        keys_pkt.extend_from_slice(&[0x04, 0x00, 16, 0x00]);
        keys_pkt.extend_from_slice(&[0xBB; 16]);

        assert_eq!(
            parse_inbound(&keys_pkt).unwrap(),
            AapInbound::ProximityKeys {
                irk: Some([0xAA; 16]),
                enc_key: Some([0xBB; 16]),
            }
        );
    }

    #[test]
    fn parses_and_encodes_140_byte_pme_record_preserving_audiogram_bytes() {
        let mut pme = [0x5A_u8; PME_RECORD_LEN];
        pme[0..9].copy_from_slice(&[0x04, 0x00, 0x04, 0x00, 0x53, 0x00, 0x84, 0x00, 0x02]);
        pme[9] = 0x01; // enrolled
        pme[10] = 0x02; // calls off
        pme[11] = 0x01; // media on

        let parsed = parse_inbound(&pme).unwrap();
        match parsed {
            AapInbound::PmeRecord {
                raw,
                enrolled,
                calls,
                media,
            } => {
                assert!(enrolled);
                assert!(!calls);
                assert!(media);
                let reencoded = encode_pme_record(&raw, true, false).unwrap();
                assert_eq!(reencoded[10], 0x01);
                assert_eq!(reencoded[11], 0x02);
                assert_eq!(&reencoded[12..], &pme[12..]);
            }
            other => panic!("expected PmeRecord, got {other:?}"),
        }
    }

    #[test]
    fn parses_all_0x09_settings() {
        assert_eq!(
            parse_inbound(&set_mic_side(MicSide::Left)).unwrap(),
            AapInbound::Setting(AapSettingUpdate::MicSide(MicSide::Left))
        );
        assert_eq!(
            parse_inbound(&set_ear_detection(false)).unwrap(),
            AapInbound::Setting(AapSettingUpdate::EarDetection(false))
        );
        assert_eq!(
            parse_inbound(&set_listening_mode(AapListeningMode::Adaptive)).unwrap(),
            AapInbound::Setting(AapSettingUpdate::ListeningMode(AapListeningMode::Adaptive))
        );
        assert_eq!(
            parse_inbound(&set_press_speed(PressSpeed::Slowest)).unwrap(),
            AapInbound::Setting(AapSettingUpdate::PressSpeed(PressSpeed::Slowest))
        );
        assert_eq!(
            parse_inbound(&set_hold_duration(HoldDuration::Shortest)).unwrap(),
            AapInbound::Setting(AapSettingUpdate::HoldDuration(HoldDuration::Shortest))
        );
        assert_eq!(
            parse_inbound(&set_stem_modes(
                StemModeMask::NOISE_CANCELING | StemModeMask::TRANSPARENCY
            ))
            .unwrap(),
            AapInbound::Setting(AapSettingUpdate::StemModes(
                StemModeMask::NOISE_CANCELING | StemModeMask::TRANSPARENCY
            ))
        );
        assert_eq!(
            parse_inbound(&set_one_bud_anc(true)).unwrap(),
            AapInbound::Setting(AapSettingUpdate::OneBudAnc(true))
        );
        assert_eq!(
            parse_inbound(&[0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x1F, 150, 0x50, 0, 0]).unwrap(),
            AapInbound::Setting(AapSettingUpdate::ToneVolume(100))
        );
        assert_eq!(
            parse_inbound(&set_personalized_volume(true)).unwrap(),
            AapInbound::Setting(AapSettingUpdate::PersonalizedVolume(true))
        );
        assert_eq!(
            parse_inbound(&set_conversation_awareness(true)).unwrap(),
            AapInbound::Setting(AapSettingUpdate::ConversationAwareness(true))
        );
        assert_eq!(
            parse_inbound(&set_hearing_aid(true)).unwrap(),
            AapInbound::Setting(AapSettingUpdate::HearingAid(true))
        );
        assert_eq!(
            parse_inbound(&[0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x2E, 65, 0, 0, 0]).unwrap(),
            AapInbound::Setting(AapSettingUpdate::AdaptiveStrength(65))
        );
        assert_eq!(
            parse_inbound(&set_hearing_assist(true)).unwrap(),
            AapInbound::Setting(AapSettingUpdate::HearingAssist(true))
        );
        assert_eq!(
            parse_inbound(&set_allow_off(false)).unwrap(),
            AapInbound::Setting(AapSettingUpdate::AllowOff(false))
        );
    }
}
