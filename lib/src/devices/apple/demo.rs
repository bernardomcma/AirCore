use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use macaddr::MacAddr6;
use openscq30_i18n::Translate;
use tokio::sync::{mpsc, watch};

use crate::{
    api::{
        connection::{self, AapBackend, AapConnection, ConnectionDescriptor, ConnectionStatus},
        device::{self, OpenSCQ30Device, OpenSCQ30DeviceRegistry},
    },
    devices::DeviceModel,
    storage::OpenSCQ30Database,
};

use super::{
    aap::{self, AapInbound, AapSettingUpdate, PME_RECORD_LEN},
    airpods_pro_2::AirPodsPro2Registry,
    device::AirPodsPro2Config,
    state::{AapListeningMode, HoldDuration, MicSide, PressSpeed, StemModeMask},
};

pub struct DemoAirPodsPro2Registry {
    inner: AirPodsPro2Registry,
}

impl DemoAirPodsPro2Registry {
    pub fn new(database: Arc<OpenSCQ30Database>) -> Self {
        Self {
            inner: AirPodsPro2Registry::new(
                Arc::new(DemoAapBackend),
                database,
                AirPodsPro2Config::default(),
            ),
        }
    }
}

#[async_trait]
impl OpenSCQ30DeviceRegistry for DemoAirPodsPro2Registry {
    async fn devices(&self) -> device::Result<Vec<ConnectionDescriptor>> {
        self.inner.devices().await
    }

    async fn connect(
        &self,
        mac_address: MacAddr6,
    ) -> device::Result<Arc<dyn OpenSCQ30Device + Send + Sync>> {
        self.inner.connect(mac_address).await
    }
}

struct DemoAapBackend;

#[async_trait]
impl AapBackend for DemoAapBackend {
    async fn devices(&self) -> connection::Result<HashSet<ConnectionDescriptor>> {
        Ok(HashSet::from([ConnectionDescriptor {
            name: DeviceModel::AppleAirPodsPro2.translate(),
            mac_address: DeviceModel::AppleAirPodsPro2.demo_mac_address(),
        }]))
    }

    async fn connect(
        &self,
        _mac_address: MacAddr6,
    ) -> connection::Result<Arc<dyn AapConnection + Send + Sync>> {
        Ok(Arc::new(DemoAapConnection::new()))
    }
}

struct DemoPodState {
    listening_mode: AapListeningMode,
    allow_off: bool,
    one_bud_anc: bool,
    stem_modes: StemModeMask,
    press_speed: PressSpeed,
    hold_duration: HoldDuration,
    tone_volume: u8,
    personalized_volume: bool,
    conversation_awareness: bool,
    ear_detection: bool,
    mic_side: MicSide,
    hearing_aid: bool,
    hearing_assist: bool,
    adaptive_strength: u8,
    pme_record: [u8; PME_RECORD_LEN],
}

impl Default for DemoPodState {
    fn default() -> Self {
        let mut pme_record = [0_u8; PME_RECORD_LEN];
        pme_record[0..9].copy_from_slice(&[0x04, 0x00, 0x04, 0x00, 0x53, 0x00, 0x84, 0x00, 0x02]);
        pme_record[9] = 0x01; // enrolled
        pme_record[10] = 0x02; // calls off
        pme_record[11] = 0x02; // media off
        Self {
            listening_mode: AapListeningMode::Adaptive,
            allow_off: true,
            one_bud_anc: false,
            stem_modes: StemModeMask::NOISE_CANCELING
                | StemModeMask::TRANSPARENCY
                | StemModeMask::ADAPTIVE,
            press_speed: PressSpeed::Default,
            hold_duration: HoldDuration::Default,
            tone_volume: 80,
            personalized_volume: true,
            conversation_awareness: true,
            ear_detection: true,
            mic_side: MicSide::Automatic,
            hearing_aid: false,
            hearing_assist: false,
            adaptive_strength: 50,
            pme_record,
        }
    }
}

pub struct DemoAapConnection {
    connection_status: watch::Sender<ConnectionStatus>,
    inbound_tx: mpsc::Sender<Vec<u8>>,
    inbound_rx: Mutex<Option<mpsc::Receiver<Vec<u8>>>>,
    state: tokio::sync::Mutex<DemoPodState>,
}

impl DemoAapConnection {
    pub fn new() -> Self {
        let (connection_status, _) = watch::channel(ConnectionStatus::Connected);
        let (inbound_tx, inbound_rx) = mpsc::channel(128);
        Self {
            connection_status,
            inbound_tx,
            inbound_rx: Mutex::new(Some(inbound_rx)),
            state: tokio::sync::Mutex::new(DemoPodState::default()),
        }
    }

    fn device_info_packet() -> Vec<u8> {
        let mut pkt = vec![
            0x04, 0x00, 0x04, 0x00, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        pkt.extend_from_slice(b"AirPods Pro 2\0A3048\0Apple Inc.\0DEMO000A3048\0");
        pkt
    }

    fn initial_battery_packet() -> Vec<u8> {
        vec![
            0x04, 0x00, 0x04, 0x00, 0x04, 0x00, 0x03, // count = 3
            0x04, 0x01, 85, 0x02, 0x01, // Left 85% discharging
            0x02, 0x01, 90, 0x02, 0x01, // Right 90% discharging
            0x08, 0x01, 70, 0x05, 0x01, // Case 70% optimized charging
        ]
    }

    fn proximity_keys_packet() -> Vec<u8> {
        let mut pkt = vec![0x04, 0x00, 0x04, 0x00, 0x31, 0x00, 0x02];
        pkt.extend_from_slice(&[0x01, 0x00, 16, 0x00]);
        pkt.extend_from_slice(&[0x11; 16]);
        pkt.extend_from_slice(&[0x04, 0x00, 16, 0x00]);
        pkt.extend_from_slice(&[0x22; 16]);
        pkt
    }
}

#[async_trait]
impl AapConnection for DemoAapConnection {
    async fn write(&self, data: &[u8]) -> connection::Result<()> {
        if data == aap::handshake() {
            let _ = self.inbound_tx.send(Self::device_info_packet()).await;
            return Ok(());
        }
        if data == aap::feature_flags() {
            return Ok(());
        }
        if data.len() >= 6 && data[0..6] == [0x04, 0x00, 0x04, 0x00, 0x0F, 0x00] {
            let s = self.state.lock().await;
            let packets = vec![
                vec![0x04, 0x00, 0x04, 0x00, 0x08, 0x00, 0x01], // Left primary
                vec![0x04, 0x00, 0x04, 0x00, 0x06, 0x00, 0x00, 0x00], // Both InEar
                Self::initial_battery_packet(),
                s.pme_record.to_vec(),
                aap::set_listening_mode(s.listening_mode),
                aap::set_press_speed(s.press_speed),
                aap::set_hold_duration(s.hold_duration),
                aap::set_one_bud_anc(s.one_bud_anc),
                aap::set_tone_volume(s.tone_volume).unwrap(),
                aap::set_personalized_volume(s.personalized_volume),
                aap::set_conversation_awareness(s.conversation_awareness),
                aap::set_hearing_aid(s.hearing_aid),
                vec![
                    0x04,
                    0x00,
                    0x04,
                    0x00,
                    0x09,
                    0x00,
                    0x2E,
                    s.adaptive_strength,
                    0x00,
                    0x00,
                    0x00,
                ],
                aap::set_hearing_assist(s.hearing_assist),
                aap::set_allow_off(s.allow_off),
            ];
            drop(s);
            for pkt in packets {
                let _ = self.inbound_tx.send(pkt).await;
            }
            return Ok(());
        }
        if data == aap::request_proximity_keys() {
            let _ = self.inbound_tx.send(Self::proximity_keys_packet()).await;
            return Ok(());
        }

        if let Some(parsed) = aap::parse_inbound(data) {
            match parsed {
                AapInbound::Setting(update) => {
                    let mut s = self.state.lock().await;
                    match update {
                        AapSettingUpdate::MicSide(v) => s.mic_side = v,
                        AapSettingUpdate::EarDetection(v) => s.ear_detection = v,
                        AapSettingUpdate::ListeningMode(v) => s.listening_mode = v,
                        AapSettingUpdate::PressSpeed(v) => s.press_speed = v,
                        AapSettingUpdate::HoldDuration(v) => s.hold_duration = v,
                        AapSettingUpdate::StemModes(v) => s.stem_modes = v,
                        AapSettingUpdate::OneBudAnc(v) => s.one_bud_anc = v,
                        AapSettingUpdate::ToneVolume(v) => s.tone_volume = v,
                        AapSettingUpdate::PersonalizedVolume(v) => s.personalized_volume = v,
                        AapSettingUpdate::ConversationAwareness(v) => {
                            s.conversation_awareness = v;
                        }
                        AapSettingUpdate::HearingAid(v) => s.hearing_aid = v,
                        AapSettingUpdate::AdaptiveStrength(v) => s.adaptive_strength = v,
                        AapSettingUpdate::HearingAssist(v) => s.hearing_assist = v,
                        AapSettingUpdate::AllowOff(v) => s.allow_off = v,
                    }
                    drop(s);
                    let _ = self.inbound_tx.send(data.to_vec()).await;
                }
                AapInbound::PmeRecord { raw, .. } => {
                    // Hardware does NOT echo 0x53 PME record back!
                    self.state.lock().await.pme_record = *raw;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn read_channel(&self) -> mpsc::Receiver<Vec<u8>> {
        self.inbound_rx
            .lock()
            .unwrap()
            .take()
            .expect("DemoAapConnection::read_channel should only be called once")
    }

    fn connection_status(&self) -> watch::Receiver<ConnectionStatus> {
        self.connection_status.subscribe()
    }
}
