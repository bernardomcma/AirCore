use std::{borrow::Cow, panic::Location, sync::Arc, time::Duration};

use async_trait::async_trait;
use macaddr::MacAddr6;
use openscq30_i18n::Translate;
use tokio::{
    sync::{Mutex, Notify, RwLock, Semaphore, mpsc, watch},
    time::Instant,
};
use tracing::{debug, warn};

use crate::{
    api::{
        connection::{self, AapBackend, AapConnection, ConnectionStatus},
        device::{self, OpenSCQ30Device},
        settings::{self, CategoryId, Range, Select, Setting, SettingId, Value},
    },
    devices::DeviceModel,
    i18n::fl,
    storage::OpenSCQ30Database,
};

use super::{
    aap::{self, AapInbound, AapSettingUpdate},
    state::{
        AapListeningMode, AirPodsPro2State, ConversationAwarenessState, HoldDuration, MicSide,
        PressSpeed, SessionPhase, StemModeMask, sanitize_stem_modes, validate_user_stem_modes,
    },
};

const KEY_ALLOW_OFF: &str = "allow_off";
const KEY_STEM_MODES: &str = "stem_modes";
const KEY_EAR_DETECTION: &str = "ear_detection";
const KEY_MIC_SIDE: &str = "mic_side";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AirPodsPro2Config {
    pub handshake_timeout: Duration,
    pub post_handshake_delay: Duration,
    pub initial_report_timeout: Duration,
    pub initial_report_window: Duration,
    pub initial_connect_sync_timeout: Duration,
    pub max_reattach_attempts: u8,
    pub proximity_key_retry_delay: Duration,
    pub listening_mode_quiet_window: Duration,
    pub hearing_assist_delay: Duration,
    pub ca_quiet_restore_delay: Duration,
}

impl Default for AirPodsPro2Config {
    fn default() -> Self {
        Self {
            handshake_timeout: Duration::from_secs(10),
            post_handshake_delay: Duration::from_millis(100),
            initial_report_timeout: Duration::from_secs(60),
            initial_report_window: Duration::from_secs(1),
            initial_connect_sync_timeout: Duration::from_secs(2),
            max_reattach_attempts: 3,
            proximity_key_retry_delay: Duration::from_secs(3),
            listening_mode_quiet_window: Duration::from_millis(3000),
            hearing_assist_delay: Duration::from_millis(300),
            ca_quiet_restore_delay: Duration::from_millis(2000),
        }
    }
}

pub struct AirPodsPro2Device {
    mac_address: MacAddr6,
    database: Arc<OpenSCQ30Database>,
    config: AirPodsPro2Config,
    active_conn: Arc<RwLock<Arc<dyn AapConnection + Send + Sync>>>,
    state_tx: watch::Sender<AirPodsPro2State>,
    device_status_tx: watch::Sender<ConnectionStatus>,
    write_lock: Arc<Mutex<()>>,
    state_wake: Arc<Notify>,
    exit_signal: Arc<Semaphore>,
}

impl Drop for AirPodsPro2Device {
    fn drop(&mut self) {
        self.exit_signal.close();
    }
}

struct AttachParts {
    conn: Arc<dyn AapConnection + Send + Sync>,
    inbound_rx: mpsc::Receiver<Vec<u8>>,
    conn_status_rx: watch::Receiver<ConnectionStatus>,
    call_audio_rx: Option<watch::Receiver<bool>>,
}

impl AirPodsPro2Device {
    pub async fn connect(
        backend: Arc<dyn AapBackend + Send + Sync>,
        database: Arc<OpenSCQ30Database>,
        mac_address: MacAddr6,
        config: AirPodsPro2Config,
    ) -> device::Result<Self> {
        let mut initial_state = AirPodsPro2State::default();
        let persisted = database.fetch_device_settings(mac_address).await?;
        apply_persisted_settings(&mut initial_state, &persisted);

        let (state_tx, _) = watch::channel(initial_state);
        let parts = perform_attach(backend.as_ref(), mac_address, config, &state_tx).await?;

        let active_conn = Arc::new(RwLock::new(parts.conn.clone()));
        let (device_status_tx, _) = watch::channel(ConnectionStatus::Connected);
        let (initial_sync_tx, mut initial_sync_rx) = watch::channel(false);
        let write_lock = Arc::new(Mutex::new(()));
        let state_wake = Arc::new(Notify::new());
        let exit_signal = Arc::new(Semaphore::new(0));

        tokio::spawn(run_session_loop(
            backend,
            mac_address,
            config,
            parts,
            active_conn.clone(),
            state_tx.clone(),
            device_status_tx.clone(),
            initial_sync_tx,
            write_lock.clone(),
            state_wake.clone(),
            exit_signal.clone(),
        ));

        tokio::select! {
            _ = initial_sync_rx.wait_for(|&done| done) => {}
            () = tokio::time::sleep(config.initial_connect_sync_timeout) => {}
        }

        Ok(Self {
            mac_address,
            database,
            config,
            active_conn,
            state_tx,
            device_status_tx,
            write_lock,
            state_wake,
            exit_signal,
        })
    }

    #[cfg(test)]
    pub fn state(&self) -> AirPodsPro2State {
        self.state_tx.borrow().clone()
    }

    async fn send_packet(&self, bytes: &[u8]) -> device::Result<()> {
        let conn = self.active_conn.read().await.clone();
        conn.write(bytes).await?;
        Ok(())
    }

    async fn persist_setting(&self, key: &str, value: String) -> device::Result<()> {
        self.database
            .upsert_device_setting(self.mac_address, key.to_owned(), value)
            .await?;
        Ok(())
    }
}

fn apply_persisted_settings(
    state: &mut AirPodsPro2State,
    persisted: &std::collections::HashMap<String, String>,
) {
    if let Some(val) = persisted.get(KEY_ALLOW_OFF)
        && let Ok(parsed) = val.parse::<bool>()
    {
        state.persisted.allow_off = Some(parsed);
    }
    if let Some(val) = persisted.get(KEY_STEM_MODES)
        && let Ok(bits) = val.parse::<u8>()
    {
        state.persisted.stem_modes = Some(StemModeMask::from_bits_truncate(bits));
    }
    if let Some(val) = persisted.get(KEY_EAR_DETECTION)
        && let Ok(parsed) = val.parse::<bool>()
    {
        state.persisted.ear_detection = Some(parsed);
    }
    if let Some(val) = persisted.get(KEY_MIC_SIDE)
        && let Ok(parsed) = val.parse::<MicSide>()
    {
        state.persisted.mic_side = Some(parsed);
    }
    state.restore_persisted_host_settings();
}

fn apply_raw_packet(
    state_tx: &watch::Sender<AirPodsPro2State>,
    pkt: &[u8],
    quiet_window: Duration,
) -> Option<AapInbound> {
    let parsed = aap::parse_inbound(pkt)?;
    let now = Instant::now();
    let cloned = parsed.clone();
    state_tx.send_modify(|s| {
        s.apply_inbound(parsed, now, quiet_window);
    });
    Some(cloned)
}

async fn perform_attach(
    backend: &(dyn AapBackend + Send + Sync),
    mac_address: MacAddr6,
    config: AirPodsPro2Config,
    state_tx: &watch::Sender<AirPodsPro2State>,
) -> device::Result<AttachParts> {
    let conn = backend.connect(mac_address).await?;
    let mut inbound_rx = conn.read_channel();
    let conn_status_rx = conn.connection_status();
    let call_audio_rx = conn.call_audio_active();

    state_tx.send_modify(|s| {
        s.phase = SessionPhase::HandshakeSent;
        s.saw_any_0x09 = false;
    });

    conn.write(&aap::handshake()).await?;

    let handshake_deadline = Instant::now() + config.handshake_timeout;
    tokio::select! {
        () = tokio::time::sleep_until(handshake_deadline) => {
            return Err(device::Error::ActionTimedOut {
                action: "aap handshake",
            });
        }
        maybe_pkt = inbound_rx.recv() => {
            match maybe_pkt {
                Some(pkt) => {
                    let _ = apply_raw_packet(state_tx, &pkt, config.listening_mode_quiet_window);
                }
                None => {
                    return Err(device::Error::ConnectionError {
                        source: connection::Error::Other {
                            source: std::io::Error::new(
                                std::io::ErrorKind::UnexpectedEof,
                                "AAP connection closed before handshake reply",
                            )
                            .into(),
                            location: Location::caller(),
                        },
                        location: Location::caller(),
                    });
                }
            }
        }
    }

    let post_delay_deadline = Instant::now() + config.post_handshake_delay;
    loop {
        tokio::select! {
            () = tokio::time::sleep_until(post_delay_deadline) => break,
            maybe_pkt = inbound_rx.recv() => {
                if let Some(pkt) = maybe_pkt {
                    let _ = apply_raw_packet(state_tx, &pkt, config.listening_mode_quiet_window);
                } else {
                    break;
                }
            }
        }
    }

    conn.write(&aap::feature_flags()).await?;
    let ear_detection = state_tx.borrow().ear_detection;
    conn.write(&aap::request_notifications(ear_detection))
        .await?;
    state_tx.send_modify(|s| s.phase = SessionPhase::Ready);

    Ok(AttachParts {
        conn,
        inbound_rx,
        conn_status_rx,
        call_audio_rx,
    })
}

async fn replay_host_settings(
    conn: &(dyn AapConnection + Send + Sync),
    state_tx: &watch::Sender<AirPodsPro2State>,
) -> connection::Result<()> {
    let mut replay_values = None;
    state_tx.send_modify(|s| {
        s.restore_persisted_host_settings();
        replay_values = Some((s.allow_off, s.stem_modes, s.ear_detection, s.mic_side));
    });
    let (allow_off, stem_modes, ear_detection, mic_side) =
        replay_values.expect("send_modify closure always initializes replay_values");

    conn.write(&aap::set_allow_off(allow_off)).await?;
    conn.write(&aap::set_stem_modes(stem_modes)).await?;
    conn.write(&aap::set_ear_detection(ear_detection)).await?;
    conn.write(&aap::set_mic_side(mic_side)).await?;
    Ok(())
}

async fn sleep_until_opt(deadline: Option<Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d).await,
        None => std::future::pending::<()>().await,
    }
}

async fn recv_call_audio_opt(rx: &mut Option<watch::Receiver<bool>>) -> bool {
    match rx {
        Some(receiver) => receiver.changed().await.is_ok(),
        None => std::future::pending::<bool>().await,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_session_loop(
    backend: Arc<dyn AapBackend + Send + Sync>,
    mac_address: MacAddr6,
    config: AirPodsPro2Config,
    mut parts: AttachParts,
    active_conn: Arc<RwLock<Arc<dyn AapConnection + Send + Sync>>>,
    state_tx: watch::Sender<AirPodsPro2State>,
    device_status_tx: watch::Sender<ConnectionStatus>,
    initial_sync_tx: watch::Sender<bool>,
    write_lock: Arc<Mutex<()>>,
    state_wake: Arc<Notify>,
    exit_signal: Arc<Semaphore>,
) {
    let mut reattach_count = 0_u8;
    let mut proximity_retry_deadline: Option<Instant> = None;
    let mut ca_quiet_deadline: Option<Instant> = None;

    // Phase A: Wait for initial 0x09 report with reattach fallback
    loop {
        if state_tx.borrow().saw_any_0x09 {
            let window_deadline = Instant::now() + config.initial_report_window;
            loop {
                tokio::select! {
                    _ = exit_signal.acquire() => return,
                    () = tokio::time::sleep_until(window_deadline) => break,
                    maybe_pkt = parts.inbound_rx.recv() => {
                        match maybe_pkt {
                            Some(pkt) => {
                                if let Some(parsed) = apply_raw_packet(&state_tx, &pkt, config.listening_mode_quiet_window) {
                                    update_ca_deadline(&parsed, &mut ca_quiet_deadline, config.ca_quiet_restore_delay);
                                }
                            }
                            None => {
                                mark_disconnected(&state_tx, &device_status_tx);
                                let _ = initial_sync_tx.send_replace(true);
                                return;
                            }
                        }
                    }
                }
            }

            {
                let _guard = write_lock.lock().await;
                state_tx.send_modify(|s| s.phase = SessionPhase::InitialReportDone);
                if let Err(err) = replay_host_settings(parts.conn.as_ref(), &state_tx).await {
                    warn!("failed to replay host-persisted AirPods settings: {err}");
                }
            }
            let _ = initial_sync_tx.send_replace(true);
            if let Err(err) = parts.conn.write(&aap::request_proximity_keys()).await {
                warn!("failed to request proximity keys: {err}");
            } else {
                proximity_retry_deadline = Some(Instant::now() + config.proximity_key_retry_delay);
            }
            break;
        }

        let report_deadline = Instant::now() + config.initial_report_timeout;
        let mut timed_out = false;

        loop {
            if state_tx.borrow().saw_any_0x09 {
                break;
            }
            tokio::select! {
                _ = exit_signal.acquire() => return,
                () = tokio::time::sleep_until(report_deadline) => {
                    timed_out = true;
                    break;
                }
                maybe_pkt = parts.inbound_rx.recv() => {
                    match maybe_pkt {
                        Some(pkt) => {
                            if let Some(parsed) = apply_raw_packet(&state_tx, &pkt, config.listening_mode_quiet_window) {
                                update_ca_deadline(&parsed, &mut ca_quiet_deadline, config.ca_quiet_restore_delay);
                            }
                        }
                        None => {
                            mark_disconnected(&state_tx, &device_status_tx);
                            let _ = initial_sync_tx.send_replace(true);
                            return;
                        }
                    }
                }
                res = parts.conn_status_rx.changed() => {
                    if res.is_err() || *parts.conn_status_rx.borrow_and_update() == ConnectionStatus::Disconnected {
                        mark_disconnected(&state_tx, &device_status_tx);
                        let _ = initial_sync_tx.send_replace(true);
                        return;
                    }
                }
            }
        }

        if timed_out {
            let mut reattached = false;
            while reattach_count < config.max_reattach_attempts {
                reattach_count += 1;
                debug!(
                    "no 0x09 report within {:?}, attempting AAP reattach {reattach_count}/{}",
                    config.initial_report_timeout, config.max_reattach_attempts
                );
                let attach_res = tokio::select! {
                    _ = exit_signal.acquire() => return,
                    res = perform_attach(backend.as_ref(), mac_address, config, &state_tx) => res,
                };
                match attach_res {
                    Ok(new_parts) => {
                        *active_conn.write().await = new_parts.conn.clone();
                        parts = new_parts;
                        reattached = true;
                        break;
                    }
                    Err(err) => {
                        warn!("AAP reattach attempt {reattach_count} failed: {err}");
                    }
                }
            }
            if reattached {
                continue;
            }
            warn!("AAP attach gave up after {reattach_count} reattach attempts");
            state_tx.send_modify(|s| s.phase = SessionPhase::AttachGaveUp);
            let _ = initial_sync_tx.send_replace(true);
            break;
        }
    }

    // Phase B: Steady-state packet and timer loop
    loop {
        let listening_expiry = state_tx
            .borrow()
            .next_optimistic_expiry(config.listening_mode_quiet_window);

        tokio::select! {
            _ = exit_signal.acquire() => break,
            () = state_wake.notified() => {},
            maybe_pkt = parts.inbound_rx.recv() => {
                match maybe_pkt {
                    Some(pkt) => {
                        if let Some(parsed) = apply_raw_packet(&state_tx, &pkt, config.listening_mode_quiet_window) {
                            update_ca_deadline(&parsed, &mut ca_quiet_deadline, config.ca_quiet_restore_delay);
                            if matches!(parsed, AapInbound::ProximityKeys { .. }) {
                                proximity_retry_deadline = None;
                            }
                        }
                    }
                    None => {
                        mark_disconnected(&state_tx, &device_status_tx);
                        break;
                    }
                }
            }
            res = parts.conn_status_rx.changed() => {
                if res.is_err() || *parts.conn_status_rx.borrow_and_update() == ConnectionStatus::Disconnected {
                    mark_disconnected(&state_tx, &device_status_tx);
                    break;
                }
            }
            ok = recv_call_audio_opt(&mut parts.call_audio_rx) => {
                if ok {
                    if let Some(rx) = &mut parts.call_audio_rx {
                        let active = *rx.borrow_and_update();
                        state_tx.send_modify(|s| s.call_audio_active = active);
                    }
                } else {
                    parts.call_audio_rx = None;
                }
            }
            () = sleep_until_opt(proximity_retry_deadline) => {
                proximity_retry_deadline = None;
                let missing_keys = {
                    let s = state_tx.borrow();
                    s.proximity_keys.irk.is_none() || s.proximity_keys.enc_key.is_none()
                };
                if missing_keys {
                    let conn = active_conn.read().await.clone();
                    if let Err(err) = conn.write(&aap::request_proximity_keys()).await {
                        warn!("proximity key retry failed: {err}");
                    }
                }
            }
            () = sleep_until_opt(listening_expiry) => {
                let now = Instant::now();
                state_tx.send_if_modified(|s| {
                    s.prune_expired_optimistic_modes(now, config.listening_mode_quiet_window)
                });
            }
            () = sleep_until_opt(ca_quiet_deadline) => {
                ca_quiet_deadline = None;
                state_tx.send_if_modified(|s| {
                    if s.ca_state != ConversationAwarenessState::Inactive {
                        s.ca_state = ConversationAwarenessState::Inactive;
                        true
                    } else {
                        false
                    }
                });
            }
        }
    }
}

fn update_ca_deadline(
    parsed: &AapInbound,
    ca_quiet_deadline: &mut Option<Instant>,
    ca_restore_delay: Duration,
) {
    match parsed {
        AapInbound::CaSpeakingLevel(1..=2)
        | AapInbound::CaSpeakingLevel(8..=9)
        | AapInbound::Setting(AapSettingUpdate::ConversationAwareness(false)) => {
            *ca_quiet_deadline = None;
        }
        AapInbound::CaSpeakingLevel(3..=7) => {
            *ca_quiet_deadline = Some(Instant::now() + ca_restore_delay);
        }
        _ => {}
    }
}

fn mark_disconnected(
    state_tx: &watch::Sender<AirPodsPro2State>,
    device_status_tx: &watch::Sender<ConnectionStatus>,
) {
    state_tx.send_modify(|s| {
        s.phase = SessionPhase::Disconnected;
        s.ca_state = ConversationAwarenessState::Inactive;
    });
    device_status_tx.send_replace(ConnectionStatus::Disconnected);
}

fn setting_error(setting_id: SettingId, message: impl Into<String>) -> device::Error {
    settings::Error {
        setting_id,
        source: std::io::Error::other(message.into()).into(),
    }
    .into()
}

#[async_trait]
impl OpenSCQ30Device for AirPodsPro2Device {
    fn connection_status(&self) -> watch::Receiver<ConnectionStatus> {
        self.device_status_tx.subscribe()
    }

    fn model(&self) -> DeviceModel {
        DeviceModel::AppleAirPodsPro2
    }

    fn categories(&self) -> Vec<CategoryId> {
        vec![
            CategoryId::SoundModes,
            CategoryId::General,
            CategoryId::ButtonConfiguration,
            CategoryId::HearingAssistance,
            CategoryId::DeviceInformation,
        ]
    }

    fn settings_in_category(&self, category_id: &CategoryId) -> Vec<SettingId> {
        match category_id {
            CategoryId::SoundModes => vec![
                SettingId::ListeningMode,
                SettingId::AllowOffListeningMode,
                SettingId::OneBudAnc,
                SettingId::AdaptiveAudioStrength,
            ],
            CategoryId::General => vec![
                SettingId::EarDetection,
                SettingId::LeftEarStatus,
                SettingId::RightEarStatus,
                SettingId::MicSide,
                SettingId::ConversationAwareness,
                SettingId::ConversationAwarenessSpeaking,
                SettingId::PersonalizedVolume,
                SettingId::ToneVolume,
            ],
            CategoryId::ButtonConfiguration => vec![
                SettingId::StemCycleModes,
                SettingId::PressSpeed,
                SettingId::HoldDuration,
            ],
            CategoryId::HearingAssistance => vec![
                SettingId::HearingAid,
                SettingId::MediaAssistCalls,
                SettingId::MediaAssistMedia,
            ],
            CategoryId::DeviceInformation => vec![
                SettingId::BatteryLevelLeft,
                SettingId::IsChargingLeft,
                SettingId::BatteryLevelRight,
                SettingId::IsChargingRight,
                SettingId::CaseBatteryLevel,
                SettingId::IsChargingCase,
                SettingId::DeviceName,
                SettingId::ModelNumber,
                SettingId::Manufacturer,
                SettingId::SerialNumber,
            ],
            _ => Vec::new(),
        }
    }

    fn setting(&self, setting_id: &SettingId) -> Option<Setting> {
        let state = self.state_tx.borrow();
        let now = Instant::now();
        match setting_id {
            SettingId::ListeningMode => {
                let modes = state.available_listening_modes();
                let current =
                    state.effective_listening_mode(now, self.config.listening_mode_quiet_window);
                Some(Setting::select_from_enum(&modes, current))
            }
            SettingId::AllowOffListeningMode => Some(Setting::Toggle {
                value: state.allow_off,
            }),
            SettingId::OneBudAnc => state.one_bud_anc.map(|value| Setting::Toggle { value }),
            SettingId::AdaptiveAudioStrength => {
                state
                    .adaptive_strength
                    .map(|strength| Setting::Information {
                        value: format!("{strength}/100"),
                        translated_value: fl!("percent", percent = i32::from(strength)),
                    })
            }
            SettingId::EarDetection => Some(Setting::Toggle {
                value: state.ear_detection,
            }),
            SettingId::LeftEarStatus => state.left_ear().map(|status| Setting::Information {
                value: status.to_string(),
                translated_value: status.translate(),
            }),
            SettingId::RightEarStatus => state.right_ear().map(|status| Setting::Information {
                value: status.to_string(),
                translated_value: status.translate(),
            }),
            SettingId::MicSide => Some(Setting::select_from_enum_all_variants(state.mic_side)),
            SettingId::ConversationAwareness => Some(Setting::Toggle {
                value: state.conversation_awareness,
            }),
            SettingId::ConversationAwarenessSpeaking => {
                if !state.conversation_awareness {
                    return None;
                }
                Some(Setting::Information {
                    value: state.ca_state.to_string(),
                    translated_value: state.ca_state.translate(),
                })
            }
            SettingId::PersonalizedVolume => Some(Setting::Toggle {
                value: state.personalized_volume,
            }),
            SettingId::ToneVolume => Some(Setting::I32Range {
                setting: Range {
                    range: 0..=100,
                    step: 1,
                },
                value: i32::from(state.tone_volume),
            }),
            SettingId::StemCycleModes => {
                let available = state.available_listening_modes();
                let values = state
                    .stem_modes
                    .to_listening_modes()
                    .into_iter()
                    .filter(|mode| available.contains(mode))
                    .map(|mode| Cow::Borrowed(mode.into()))
                    .collect();
                Some(Setting::MultiSelect {
                    setting: Select::from_enum(&available),
                    values,
                })
            }
            SettingId::PressSpeed => {
                Some(Setting::select_from_enum_all_variants(state.press_speed))
            }
            SettingId::HoldDuration => {
                Some(Setting::select_from_enum_all_variants(state.hold_duration))
            }
            SettingId::HearingAid => {
                if state.hearing_aid_enrolled != Some(true) {
                    return None;
                }
                Some(Setting::Toggle {
                    value: state.hearing_aid_enabled,
                })
            }
            SettingId::MediaAssistCalls => {
                if state.hearing_aid_enrolled != Some(true) || state.pme_record.is_none() {
                    return None;
                }
                Some(Setting::Toggle {
                    value: state.media_assist_calls,
                })
            }
            SettingId::MediaAssistMedia => {
                if state.hearing_aid_enrolled != Some(true) || state.pme_record.is_none() {
                    return None;
                }
                Some(Setting::Toggle {
                    value: state.media_assist_media,
                })
            }
            SettingId::BatteryLevelLeft => {
                state.battery_left.percent.map(|pct| Setting::Information {
                    value: format!("{pct}/100"),
                    translated_value: fl!("percent", percent = i32::from(pct)),
                })
            }
            SettingId::IsChargingLeft => state.battery_left.percent.map(|_| Setting::Information {
                value: state.battery_left.charging.to_string(),
                translated_value: if state.battery_left.charging {
                    fl!("charging")
                } else {
                    fl!("not-charging")
                },
            }),
            SettingId::BatteryLevelRight => {
                state.battery_right.percent.map(|pct| Setting::Information {
                    value: format!("{pct}/100"),
                    translated_value: fl!("percent", percent = i32::from(pct)),
                })
            }
            SettingId::IsChargingRight => {
                state.battery_right.percent.map(|_| Setting::Information {
                    value: state.battery_right.charging.to_string(),
                    translated_value: if state.battery_right.charging {
                        fl!("charging")
                    } else {
                        fl!("not-charging")
                    },
                })
            }
            SettingId::CaseBatteryLevel => {
                state.battery_case.percent.map(|pct| Setting::Information {
                    value: format!("{pct}/100"),
                    translated_value: fl!("percent", percent = i32::from(pct)),
                })
            }
            SettingId::IsChargingCase => state.battery_case.percent.map(|_| Setting::Information {
                value: state.battery_case.charging.to_string(),
                translated_value: if state.battery_case.charging {
                    fl!("charging")
                } else {
                    fl!("not-charging")
                },
            }),
            SettingId::DeviceName => state.metadata.name.as_ref().map(|s| Setting::Information {
                value: s.clone(),
                translated_value: s.clone(),
            }),
            SettingId::ModelNumber => {
                state
                    .metadata
                    .model_number
                    .as_ref()
                    .map(|s| Setting::Information {
                        value: s.clone(),
                        translated_value: s.clone(),
                    })
            }
            SettingId::Manufacturer => {
                state
                    .metadata
                    .manufacturer
                    .as_ref()
                    .map(|s| Setting::Information {
                        value: s.clone(),
                        translated_value: s.clone(),
                    })
            }
            SettingId::SerialNumber => {
                state
                    .metadata
                    .serial_number
                    .as_ref()
                    .map(|s| Setting::Information {
                        value: s.clone(),
                        translated_value: s.clone(),
                    })
            }
            _ => None,
        }
    }

    fn watch_for_changes(&self) -> watch::Receiver<()> {
        let mut receiver = self.state_tx.subscribe();
        let (change_sender, change_receiver) = watch::channel(());
        tokio::spawn(async move {
            while receiver.changed().await.is_ok() {
                if change_sender.send(()).is_err() {
                    return;
                }
            }
        });
        change_receiver
    }

    async fn set_setting_values(
        &self,
        setting_values: Vec<(SettingId, Value)>,
    ) -> device::Result<()> {
        let _guard = self.write_lock.lock().await;

        let mut pending_ha: Option<bool> = None;
        let mut pending_calls: Option<bool> = None;
        let mut pending_media: Option<bool> = None;
        let mut ha_trigger_id: Option<SettingId> = None;

        for (setting_id, value) in setting_values {
            match setting_id {
                SettingId::ListeningMode => {
                    let mode: AapListeningMode =
                        value.try_as_enum_variant().map_err(|err| settings::Error {
                            setting_id,
                            source: Box::new(err),
                        })?;
                    let allow_off = self.state_tx.borrow().allow_off;
                    if mode == AapListeningMode::Off && !allow_off {
                        return Err(setting_error(
                            setting_id,
                            "Off mode is disabled when Allow Off Listening Mode is false",
                        ));
                    }
                    let now = Instant::now();
                    self.state_tx.send_modify(|s| {
                        s.record_optimistic_listening_mode(mode, now);
                    });
                    if let Err(err) = self.send_packet(&aap::set_listening_mode(mode)).await {
                        self.state_tx.send_modify(|s| {
                            s.rollback_last_optimistic_listening_mode();
                        });
                        return Err(err);
                    }
                    self.state_wake.notify_one();
                }
                SettingId::AllowOffListeningMode => {
                    let allow_off = value.try_as_bool().map_err(|err| settings::Error {
                        setting_id,
                        source: Box::new(err),
                    })?;
                    let pre_is_off = !allow_off
                        && self.state_tx.borrow().unclamped_listening_mode(
                            Instant::now(),
                            self.config.listening_mode_quiet_window,
                        ) == AapListeningMode::Off;

                    self.send_packet(&aap::set_allow_off(allow_off)).await?;
                    self.persist_setting(KEY_ALLOW_OFF, allow_off.to_string())
                        .await?;

                    let (need_stem_update, sanitized_stem, post_is_off) = {
                        let s = self.state_tx.borrow();
                        let sanitized =
                            sanitize_stem_modes(s.stem_modes, allow_off, s.capabilities.adaptive);
                        let post_is_off = !allow_off
                            && s.unclamped_listening_mode(
                                Instant::now(),
                                self.config.listening_mode_quiet_window,
                            ) == AapListeningMode::Off;
                        let has_off = s.stem_modes.contains(StemModeMask::OFF)
                            || s.persisted
                                .stem_modes
                                .is_some_and(|m| m.contains(StemModeMask::OFF));
                        (!allow_off && has_off, sanitized, post_is_off)
                    };
                    let need_mode_switch = !allow_off && (pre_is_off || post_is_off);

                    self.state_tx.send_modify(|s| {
                        s.allow_off = allow_off;
                        s.persisted.allow_off = Some(allow_off);
                    });

                    if need_stem_update {
                        self.send_packet(&aap::set_stem_modes(sanitized_stem))
                            .await?;
                        self.persist_setting(KEY_STEM_MODES, sanitized_stem.bits().to_string())
                            .await?;
                        self.state_tx.send_modify(|s| {
                            s.stem_modes = sanitized_stem;
                            s.persisted.stem_modes = Some(sanitized_stem);
                        });
                    }
                    if need_mode_switch {
                        let now = Instant::now();
                        self.state_tx.send_modify(|s| {
                            s.record_optimistic_listening_mode(AapListeningMode::Transparency, now);
                        });
                        if let Err(err) = self
                            .send_packet(&aap::set_listening_mode(AapListeningMode::Transparency))
                            .await
                        {
                            self.state_tx.send_modify(|s| {
                                s.rollback_last_optimistic_listening_mode();
                            });
                            return Err(err);
                        }
                        self.state_wake.notify_one();
                    }
                }
                SettingId::OneBudAnc => {
                    let enabled = value.try_as_bool().map_err(|err| settings::Error {
                        setting_id,
                        source: Box::new(err),
                    })?;
                    self.send_packet(&aap::set_one_bud_anc(enabled)).await?;
                    self.state_tx.send_modify(|s| s.one_bud_anc = Some(enabled));
                }
                SettingId::EarDetection => {
                    let enabled = value.try_as_bool().map_err(|err| settings::Error {
                        setting_id,
                        source: Box::new(err),
                    })?;
                    self.send_packet(&aap::set_ear_detection(enabled)).await?;
                    self.send_packet(&aap::request_notifications(enabled))
                        .await?;
                    self.persist_setting(KEY_EAR_DETECTION, enabled.to_string())
                        .await?;
                    self.state_tx.send_modify(|s| {
                        s.ear_detection = enabled;
                        s.persisted.ear_detection = Some(enabled);
                    });
                }
                SettingId::MicSide => {
                    let side: MicSide =
                        value.try_as_enum_variant().map_err(|err| settings::Error {
                            setting_id,
                            source: Box::new(err),
                        })?;
                    self.send_packet(&aap::set_mic_side(side)).await?;
                    self.persist_setting(KEY_MIC_SIDE, side.to_string()).await?;
                    self.state_tx.send_modify(|s| {
                        s.mic_side = side;
                        s.persisted.mic_side = Some(side);
                    });
                }
                SettingId::ConversationAwareness => {
                    let enabled = value.try_as_bool().map_err(|err| settings::Error {
                        setting_id,
                        source: Box::new(err),
                    })?;
                    self.send_packet(&aap::set_conversation_awareness(enabled))
                        .await?;
                    self.state_tx.send_modify(|s| {
                        s.conversation_awareness = enabled;
                        if !enabled {
                            s.ca_state = ConversationAwarenessState::Inactive;
                        }
                    });
                }
                SettingId::PersonalizedVolume => {
                    let enabled = value.try_as_bool().map_err(|err| settings::Error {
                        setting_id,
                        source: Box::new(err),
                    })?;
                    self.send_packet(&aap::set_personalized_volume(enabled))
                        .await?;
                    self.state_tx
                        .send_modify(|s| s.personalized_volume = enabled);
                }
                SettingId::ToneVolume => {
                    let raw_vol = value.try_as_i32().map_err(|err| settings::Error {
                        setting_id,
                        source: Box::new(err),
                    })?;
                    let vol = u8::try_from(raw_vol).map_err(|_| {
                        setting_error(
                            setting_id,
                            format!("tone volume {raw_vol} out of range 0..=100"),
                        )
                    })?;
                    let pkt = aap::set_tone_volume(vol)
                        .map_err(|err| setting_error(setting_id, err.to_string()))?;
                    self.send_packet(&pkt).await?;
                    self.state_tx.send_modify(|s| s.tone_volume = vol);
                }
                SettingId::StemCycleModes => {
                    let selected = value.try_into_string_vec().map_err(|err| settings::Error {
                        setting_id,
                        source: Box::new(err),
                    })?;
                    let (allow_off, adaptive_capable) = {
                        let s = self.state_tx.borrow();
                        (s.allow_off, s.capabilities.adaptive)
                    };
                    let mask = validate_user_stem_modes(&selected, allow_off, adaptive_capable)
                        .map_err(|msg| setting_error(setting_id, msg))?;
                    self.send_packet(&aap::set_stem_modes(mask)).await?;
                    self.persist_setting(KEY_STEM_MODES, mask.bits().to_string())
                        .await?;
                    self.state_tx.send_modify(|s| {
                        let sanitized =
                            sanitize_stem_modes(mask, s.allow_off, s.capabilities.adaptive);
                        s.stem_modes = sanitized;
                        s.persisted.stem_modes = Some(sanitized);
                    });
                }
                SettingId::PressSpeed => {
                    let speed: PressSpeed =
                        value.try_as_enum_variant().map_err(|err| settings::Error {
                            setting_id,
                            source: Box::new(err),
                        })?;
                    self.send_packet(&aap::set_press_speed(speed)).await?;
                    self.state_tx.send_modify(|s| s.press_speed = speed);
                }
                SettingId::HoldDuration => {
                    let dur: HoldDuration =
                        value.try_as_enum_variant().map_err(|err| settings::Error {
                            setting_id,
                            source: Box::new(err),
                        })?;
                    self.send_packet(&aap::set_hold_duration(dur)).await?;
                    self.state_tx.send_modify(|s| s.hold_duration = dur);
                }
                SettingId::HearingAid => {
                    let enabled = value.try_as_bool().map_err(|err| settings::Error {
                        setting_id,
                        source: Box::new(err),
                    })?;
                    pending_ha = Some(enabled);
                    ha_trigger_id.get_or_insert(setting_id);
                }
                SettingId::MediaAssistCalls => {
                    let enabled = value.try_as_bool().map_err(|err| settings::Error {
                        setting_id,
                        source: Box::new(err),
                    })?;
                    pending_calls = Some(enabled);
                    ha_trigger_id.get_or_insert(setting_id);
                }
                SettingId::MediaAssistMedia => {
                    let enabled = value.try_as_bool().map_err(|err| settings::Error {
                        setting_id,
                        source: Box::new(err),
                    })?;
                    pending_media = Some(enabled);
                    ha_trigger_id.get_or_insert(setting_id);
                }
                SettingId::AdaptiveAudioStrength
                | SettingId::LeftEarStatus
                | SettingId::RightEarStatus
                | SettingId::ConversationAwarenessSpeaking
                | SettingId::BatteryLevelLeft
                | SettingId::IsChargingLeft
                | SettingId::BatteryLevelRight
                | SettingId::IsChargingRight
                | SettingId::CaseBatteryLevel
                | SettingId::IsChargingCase
                | SettingId::DeviceName
                | SettingId::ModelNumber
                | SettingId::Manufacturer
                | SettingId::SerialNumber => {
                    return Err(setting_error(setting_id, "setting is read-only"));
                }
                _ => {
                    return Err(setting_error(
                        setting_id,
                        "unsupported setting for AppleAirPodsPro2",
                    ));
                }
            }
        }

        if let Some(trigger_id) = ha_trigger_id {
            let (enrolled, pme_record, old_ha, old_calls, old_media, was_assist) = {
                let s = self.state_tx.borrow();
                (
                    s.hearing_aid_enrolled,
                    s.pme_record,
                    s.hearing_aid_enabled,
                    s.media_assist_calls,
                    s.media_assist_media,
                    s.hearing_assist_active(),
                )
            };

            if enrolled != Some(true) {
                return Err(setting_error(
                    trigger_id,
                    "hearing aid is not enrolled on this device",
                ));
            }
            if (pending_calls.is_some() || pending_media.is_some()) && pme_record.is_none() {
                return Err(setting_error(
                    trigger_id,
                    "PME hearing aid record has not been received from AirPods",
                ));
            }

            let new_ha = pending_ha.unwrap_or(old_ha);
            let new_calls = pending_calls.unwrap_or(old_calls);
            let new_media = pending_media.unwrap_or(old_media);
            let want_assist = new_ha || new_calls || new_media;

            let new_pme = if new_calls != old_calls || new_media != old_media {
                let record = pme_record.ok_or_else(|| {
                    setting_error(
                        trigger_id,
                        "PME hearing aid record has not been received from AirPods",
                    )
                })?;
                Some(
                    aap::encode_pme_record(&record, new_calls, new_media)
                        .map_err(|err| setting_error(trigger_id, err.to_string()))?,
                )
            } else {
                None
            };

            if !was_assist && want_assist {
                if new_ha != old_ha {
                    self.send_packet(&aap::set_hearing_aid(new_ha)).await?;
                }
                if let Some(pme) = &new_pme {
                    self.send_packet(pme).await?;
                }
                tokio::time::sleep(self.config.hearing_assist_delay).await;
                self.send_packet(&aap::set_hearing_assist(true)).await?;
            } else if was_assist && !want_assist {
                self.send_packet(&aap::set_hearing_assist(false)).await?;
                tokio::time::sleep(self.config.hearing_assist_delay).await;
                if new_ha != old_ha {
                    self.send_packet(&aap::set_hearing_aid(false)).await?;
                }
                if let Some(pme) = &new_pme {
                    self.send_packet(pme).await?;
                }
            } else {
                if new_ha != old_ha {
                    self.send_packet(&aap::set_hearing_aid(new_ha)).await?;
                }
                if let Some(pme) = &new_pme {
                    self.send_packet(pme).await?;
                }
            }

            self.state_tx.send_modify(|s| {
                s.hearing_aid_enabled = new_ha;
                s.hearing_assist_enabled = want_assist;
                s.media_assist_calls = new_calls;
                s.media_assist_media = new_media;
                if let Some(pme) = new_pme {
                    s.pme_record = Some(pme);
                }
            });
        }

        Ok(())
    }
}
