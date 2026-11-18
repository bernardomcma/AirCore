use std::sync::Arc;

use async_trait::async_trait;
use macaddr::MacAddr6;

use crate::{
    api::{
        connection::{AapBackend, ConnectionDescriptor},
        device::{self, OpenSCQ30Device, OpenSCQ30DeviceRegistry},
    },
    storage::OpenSCQ30Database,
};

use super::device::{AirPodsPro2Config, AirPodsPro2Device};

pub struct AirPodsPro2Registry {
    backend: Arc<dyn AapBackend + Send + Sync>,
    database: Arc<OpenSCQ30Database>,
    config: AirPodsPro2Config,
}

impl AirPodsPro2Registry {
    pub fn new(
        backend: Arc<dyn AapBackend + Send + Sync>,
        database: Arc<OpenSCQ30Database>,
        config: AirPodsPro2Config,
    ) -> Self {
        Self {
            backend,
            database,
            config,
        }
    }
}

#[async_trait]
impl OpenSCQ30DeviceRegistry for AirPodsPro2Registry {
    async fn devices(&self) -> device::Result<Vec<ConnectionDescriptor>> {
        self.backend
            .devices()
            .await
            .map(|descriptors| descriptors.into_iter().collect())
            .map_err(Into::into)
    }

    async fn connect(
        &self,
        mac_address: MacAddr6,
    ) -> device::Result<Arc<dyn OpenSCQ30Device + Send + Sync>> {
        let device = AirPodsPro2Device::connect(
            self.backend.clone(),
            self.database.clone(),
            mac_address,
            self.config,
        )
        .await?;
        Ok(Arc::new(device))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        borrow::Cow,
        collections::{HashSet, VecDeque},
        sync::{
            Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use tokio::sync::{mpsc, watch};

    use crate::{
        api::{
            connection::{self, AapConnection, ConnectionStatus},
            settings::{CategoryId, Setting, SettingId, Value},
        },
        connection_backend::mock::aap::MockAapBackend,
        devices::{
            DeviceModel,
            apple::{
                aap::{self, PME_RECORD_LEN},
                demo::DemoAirPodsPro2Registry,
                state::{
                    AapListeningMode, ConversationAwarenessState, EarStatus, HoldDuration, MicSide,
                    PressSpeed, SessionPhase, StemModeMask,
                },
            },
        },
    };

    use super::*;

    struct ScriptedConn {
        inbound_rx: StdMutex<Option<mpsc::Receiver<Vec<u8>>>>,
        outbound_tx: mpsc::Sender<Vec<u8>>,
        status_tx: watch::Sender<ConnectionStatus>,
    }

    #[async_trait]
    impl AapConnection for ScriptedConn {
        async fn write(&self, data: &[u8]) -> connection::Result<()> {
            self.outbound_tx.send(data.to_vec()).await.unwrap();
            Ok(())
        }

        fn read_channel(&self) -> mpsc::Receiver<Vec<u8>> {
            self.inbound_rx.lock().unwrap().take().unwrap()
        }

        fn connection_status(&self) -> watch::Receiver<ConnectionStatus> {
            self.status_tx.subscribe()
        }
    }

    struct MultiAttachMockBackend {
        conns: StdMutex<VecDeque<Arc<dyn AapConnection + Send + Sync>>>,
    }

    impl MultiAttachMockBackend {
        fn new(conns: Vec<Arc<dyn AapConnection + Send + Sync>>) -> Self {
            Self {
                conns: StdMutex::new(VecDeque::from(conns)),
            }
        }
    }

    #[async_trait]
    impl AapBackend for MultiAttachMockBackend {
        async fn devices(&self) -> connection::Result<HashSet<ConnectionDescriptor>> {
            Ok(HashSet::new())
        }

        async fn connect(
            &self,
            _mac_address: MacAddr6,
        ) -> connection::Result<Arc<dyn AapConnection + Send + Sync>> {
            Ok(self.conns.lock().unwrap().pop_front().unwrap())
        }
    }

    fn make_conn() -> (
        Arc<ScriptedConn>,
        mpsc::Sender<Vec<u8>>,
        mpsc::Receiver<Vec<u8>>,
    ) {
        let (in_tx, in_rx) = mpsc::channel(64);
        let (out_tx, out_rx) = mpsc::channel(64);
        let (status_tx, _) = watch::channel(ConnectionStatus::Connected);
        (
            Arc::new(ScriptedConn {
                inbound_rx: StdMutex::new(Some(in_rx)),
                outbound_tx: out_tx,
                status_tx,
            }),
            in_tx,
            out_rx,
        )
    }

    #[tokio::test(start_paused = true)]
    async fn handshake_timeout_returns_action_timed_out() {
        let (_in_tx, in_rx) = mpsc::channel(10);
        let (out_tx, mut out_rx) = mpsc::channel(10);
        let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let backend = Arc::new(MockAapBackend::new(in_rx, out_tx));
        let config = AirPodsPro2Config::default();

        let connect_fut = AirPodsPro2Device::connect(backend, db, MacAddr6::nil(), config);
        let res = connect_fut.await;
        assert!(matches!(
            res,
            Err(device::Error::ActionTimedOut {
                action: "aap handshake"
            })
        ));
        assert_eq!(out_rx.recv().await.unwrap(), aap::handshake());
    }

    #[tokio::test(start_paused = true)]
    async fn no_initial_0x09_reattaches_up_to_three_times_then_gives_up() {
        let mut conns: Vec<Arc<dyn AapConnection + Send + Sync>> = Vec::new();
        let mut in_txs = Vec::new();
        let mut out_rxs = Vec::new();

        for _ in 0..4 {
            let (conn, in_tx, out_rx) = make_conn();
            conns.push(conn);
            in_txs.push(in_tx);
            out_rxs.push(out_rx);
        }

        // Auto-answer handshake for each attach with 0x1D (no 0x09!)
        for (in_tx, mut out_rx) in in_txs.into_iter().zip(out_rxs) {
            tokio::spawn(async move {
                while let Some(pkt) = out_rx.recv().await {
                    if pkt == aap::handshake() {
                        let _ = in_tx
                            .send(vec![
                                0x04, 0x00, 0x04, 0x00, 0x1D, 0x00, 0, 0, 0, 0, 0, b'A', 0,
                            ])
                            .await;
                    }
                }
            });
        }

        let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let backend = Arc::new(MultiAttachMockBackend::new(conns));
        let device =
            AirPodsPro2Device::connect(backend, db, MacAddr6::nil(), AirPodsPro2Config::default())
                .await
                .unwrap();

        // Advance past 4 * 60s timeouts (1 initial + 3 reattaches).
        // Using sleep() under start_paused lets spawned attach/responder tasks settle before advancing time.
        for _ in 0..5 {
            tokio::time::sleep(Duration::from_secs(62)).await;
        }

        assert_eq!(device.state().phase, SessionPhase::AttachGaveUp);
    }

    #[tokio::test(start_paused = true)]
    async fn initial_report_window_replays_persisted_settings_in_order_and_retries_0x30() {
        let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(64);
        let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
        let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let mac = MacAddr6::new(0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF);

        // Pre-seed persisted settings
        db.upsert_device_setting(mac, "allow_off".to_owned(), "false".to_owned())
            .await
            .unwrap();
        db.upsert_device_setting(
            mac,
            "stem_modes".to_owned(),
            (StemModeMask::OFF | StemModeMask::NOISE_CANCELING | StemModeMask::TRANSPARENCY)
                .bits()
                .to_string(),
        )
        .await
        .unwrap();
        db.upsert_device_setting(mac, "ear_detection".to_owned(), "false".to_owned())
            .await
            .unwrap();
        db.upsert_device_setting(mac, "mic_side".to_owned(), "Left".to_owned())
            .await
            .unwrap();

        let responder_tx = in_tx.clone();
        let (sent_tx, mut sent_rx) = mpsc::channel(64);
        tokio::spawn(async move {
            while let Some(pkt) = out_rx.recv().await {
                let _ = sent_tx.send(pkt.clone()).await;
                if pkt == aap::handshake() {
                    let _ = responder_tx
                        .send(vec![
                            0x04, 0x00, 0x04, 0x00, 0x1D, 0x00, 0, 0, 0, 0, 0, b'P', 0,
                        ])
                        .await;
                } else if pkt.len() >= 6 && pkt[4..6] == [0x0F, 0x00] {
                    // Send initial 0x09 setting
                    let _ = responder_tx
                        .send(aap::set_listening_mode(AapListeningMode::NoiseCanceling))
                        .await;
                }
            }
        });

        let backend = Arc::new(MockAapBackend::new(in_rx, out_tx));
        let device = AirPodsPro2Device::connect(backend, db, mac, AirPodsPro2Config::default())
            .await
            .unwrap();

        assert_eq!(device.state().phase, SessionPhase::InitialReportDone);

        let mut sent = Vec::new();
        while let Ok(pkt) = sent_rx.try_recv() {
            sent.push(pkt);
        }

        // Verify order: Handshake -> FeatureFlags -> RequestNotifications(ear_detection=false -> 0xFD)
        // -> AllowOff(false) -> StemModes(Off stripped!) -> EarDetection(false) -> MicSide(Left) -> RequestProximityKeys(0x30)
        assert_eq!(sent[0], aap::handshake());
        assert_eq!(sent[1], aap::feature_flags());
        assert_eq!(sent[2], aap::request_notifications(false));
        assert_eq!(sent[3], aap::set_allow_off(false));
        assert_eq!(
            sent[4],
            aap::set_stem_modes(StemModeMask::NOISE_CANCELING | StemModeMask::TRANSPARENCY)
        );
        assert_eq!(sent[5], aap::set_ear_detection(false));
        assert_eq!(sent[6], aap::set_mic_side(MicSide::Left));
        assert_eq!(sent[7], aap::request_proximity_keys());

        // Advance 3s without 0x31 reply -> 0x30 is retried once
        tokio::time::advance(Duration::from_secs(4)).await;
        tokio::task::yield_now().await;
        let retry_pkt = sent_rx.try_recv().unwrap();
        assert_eq!(retry_pkt, aap::request_proximity_keys());
    }

    #[tokio::test(start_paused = true)]
    async fn listening_mode_anti_flicker_and_revert() {
        let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(64);
        let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
        let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());

        let responder_tx = in_tx.clone();
        tokio::spawn(async move {
            while let Some(pkt) = out_rx.recv().await {
                if pkt == aap::handshake() {
                    let _ = responder_tx
                        .send(vec![
                            0x04, 0x00, 0x04, 0x00, 0x1D, 0x00, 0, 0, 0, 0, 0, b'P', 0,
                        ])
                        .await;
                } else if pkt.len() >= 6 && pkt[4..6] == [0x0F, 0x00] {
                    let _ = responder_tx
                        .send(aap::set_listening_mode(AapListeningMode::NoiseCanceling))
                        .await;
                }
            }
        });

        let backend = Arc::new(MockAapBackend::new(in_rx, out_tx));
        let device =
            AirPodsPro2Device::connect(backend, db, MacAddr6::nil(), AirPodsPro2Config::default())
                .await
                .unwrap();

        // User rapidly switches Transparency -> Adaptive
        device
            .set_setting_values(vec![(
                SettingId::ListeningMode,
                Value::from("Transparency"),
            )])
            .await
            .unwrap();
        device
            .set_setting_values(vec![(SettingId::ListeningMode, Value::from("Adaptive"))])
            .await
            .unwrap();

        // Stale echo for NoiseCanceling arrives -> suppressed! Still displays Adaptive
        in_tx
            .send(aap::set_listening_mode(AapListeningMode::NoiseCanceling))
            .await
            .unwrap();
        tokio::task::yield_now().await;
        assert_eq!(
            Value::from(device.setting(&SettingId::ListeningMode).unwrap()),
            Value::from("Adaptive")
        );

        // After 3000ms without Adaptive confirmation, reverts to latest device-reported (NoiseCanceling)
        tokio::time::advance(Duration::from_millis(3100)).await;
        tokio::task::yield_now().await;
        assert_eq!(
            Value::from(device.setting(&SettingId::ListeningMode).unwrap()),
            Value::from("NoiseCanceling")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn allow_off_and_stem_modes_consistency_and_validation() {
        let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let registry = DemoAirPodsPro2Registry::new(db);
        let device = registry.connect(MacAddr6::nil()).await.unwrap();

        // Reject < 2 stem cycle modes
        let err = device
            .set_setting_values(vec![(
                SettingId::StemCycleModes,
                Value::from(vec![Cow::Borrowed("NoiseCanceling")]),
            )])
            .await
            .unwrap_err();
        assert!(matches!(err, device::Error::SettingError { .. }));

        // Set ListeningMode = Off and StemCycleModes = [Off, NoiseCanceling]
        device
            .set_setting_values(vec![(
                SettingId::StemCycleModes,
                Value::from(vec![Cow::Borrowed("Off"), Cow::Borrowed("NoiseCanceling")]),
            )])
            .await
            .unwrap();
        device
            .set_setting_values(vec![(SettingId::ListeningMode, Value::from("Off"))])
            .await
            .unwrap();

        // Disabling AllowOff strips Off, auto-pads Transparency (since < 2 remain), and switches mode to Transparency
        device
            .set_setting_values(vec![(SettingId::AllowOffListeningMode, Value::from(false))])
            .await
            .unwrap();
        tokio::task::yield_now().await;

        assert_eq!(
            Value::from(device.setting(&SettingId::ListeningMode).unwrap()),
            Value::from("Transparency")
        );
        let stem = match device.setting(&SettingId::StemCycleModes).unwrap() {
            Setting::MultiSelect { values, .. } => values,
            other => panic!("expected MultiSelect, got {other:?}"),
        };
        assert!(!stem.contains(&Cow::Borrowed("Off")));
        assert!(stem.contains(&Cow::Borrowed("NoiseCanceling")));
        assert!(stem.contains(&Cow::Borrowed("Transparency")));

        // Re-enabling AllowOff must stay at Transparency (not revert to Off)
        device
            .set_setting_values(vec![(SettingId::AllowOffListeningMode, Value::from(true))])
            .await
            .unwrap();
        tokio::task::yield_now().await;
        assert_eq!(
            Value::from(device.setting(&SettingId::ListeningMode).unwrap()),
            Value::from("Transparency")
        );

        tokio::time::advance(Duration::from_millis(3100)).await;
        tokio::task::yield_now().await;
        assert_eq!(
            Value::from(device.setting(&SettingId::ListeningMode).unwrap()),
            Value::from("Transparency")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn ear_status_in_motion_preserved_and_0x08_overrides_0x04() {
        let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(64);
        let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
        let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());

        let responder_tx = in_tx.clone();
        tokio::spawn(async move {
            while let Some(pkt) = out_rx.recv().await {
                if pkt == aap::handshake() {
                    let _ = responder_tx
                        .send(vec![
                            0x04, 0x00, 0x04, 0x00, 0x1D, 0x00, 0, 0, 0, 0, 0, b'P', 0,
                        ])
                        .await;
                } else if pkt.len() >= 6 && pkt[4..6] == [0x0F, 0x00] {
                    // 0x04 fallback hints Right first
                    let _ = responder_tx
                        .send(vec![
                            0x04, 0x00, 0x04, 0x00, 0x04, 0x00, 0x02, 0x02, 0x01, 80, 0x02, 0x01,
                            0x04, 0x01, 70, 0x02, 0x01,
                        ])
                        .await;
                    let _ = responder_tx
                        .send(vec![0x04, 0x00, 0x04, 0x00, 0x06, 0x00, 0x00, 0x02])
                        .await;
                    let _ = responder_tx
                        .send(aap::set_listening_mode(AapListeningMode::Adaptive))
                        .await;
                }
            }
        });

        let backend = Arc::new(MockAapBackend::new(in_rx, out_tx));
        let device =
            AirPodsPro2Device::connect(backend, db, MacAddr6::nil(), AirPodsPro2Config::default())
                .await
                .unwrap();

        // With 0x04 fallback (Right primary): Right=InEar, Left=InCase
        assert_eq!(
            Value::from(device.setting(&SettingId::RightEarStatus).unwrap()),
            Value::from("InEar")
        );
        assert_eq!(
            Value::from(device.setting(&SettingId::LeftEarStatus).unwrap()),
            Value::from("InCase")
        );

        // Send 0x08 authoritative Left primary + 0x06 with secondary InMotion (0x04)
        in_tx
            .send(vec![0x04, 0x00, 0x04, 0x00, 0x08, 0x00, 0x01])
            .await
            .unwrap();
        in_tx
            .send(vec![0x04, 0x00, 0x04, 0x00, 0x06, 0x00, 0x01, 0x04])
            .await
            .unwrap();
        tokio::task::yield_now().await;

        // Primary (now Left) updated to OutOfEar; Secondary (now Right) preserved previous InCase!
        assert_eq!(
            Value::from(device.setting(&SettingId::LeftEarStatus).unwrap()),
            Value::from("OutOfEar")
        );
        assert_eq!(
            Value::from(device.setting(&SettingId::RightEarStatus).unwrap()),
            Value::from("InCase")
        );
        // Subsequent 0x04 with Left first doesn't override authoritative 0x08
        in_tx
            .send(vec![
                0x04, 0x00, 0x04, 0x00, 0x04, 0x00, 0x01, 0x02, 0x01, 80, 0x02, 0x01,
            ])
            .await
            .unwrap();
        tokio::task::yield_now().await;
        assert_eq!(
            device.state().primary_bud,
            Some(crate::devices::apple::state::PrimaryBud::Left)
        );
        assert_eq!(EarStatus::OutOfEar, device.state().left_ear().unwrap());
    }

    #[tokio::test(start_paused = true)]
    async fn hearing_assist_sequencing_300ms_and_enrollment_guard() {
        let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(64);
        let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
        let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());

        let (sent_tx, mut sent_rx) = mpsc::channel(64);
        let responder_tx = in_tx.clone();
        tokio::spawn(async move {
            while let Some(pkt) = out_rx.recv().await {
                let _ = sent_tx.send(pkt.clone()).await;
                if pkt == aap::handshake() {
                    let _ = responder_tx
                        .send(vec![
                            0x04, 0x00, 0x04, 0x00, 0x1D, 0x00, 0, 0, 0, 0, 0, b'P', 0,
                        ])
                        .await;
                } else if pkt.len() >= 6 && pkt[4..6] == [0x0F, 0x00] {
                    let _ = responder_tx
                        .send(aap::set_listening_mode(AapListeningMode::Adaptive))
                        .await;
                }
            }
        });

        let backend = Arc::new(MockAapBackend::new(in_rx, out_tx));
        let device =
            AirPodsPro2Device::connect(backend, db, MacAddr6::nil(), AirPodsPro2Config::default())
                .await
                .unwrap();

        // Before enrollment, HearingAid is hidden & write returns error
        assert!(device.setting(&SettingId::HearingAid).is_none());
        assert!(
            device
                .set_setting_values(vec![(SettingId::HearingAid, Value::from(true))])
                .await
                .is_err()
        );

        // Drain init packets
        while sent_rx.try_recv().is_ok() {}

        // Send enrolled 140B PME record
        let mut pme = [0x00_u8; PME_RECORD_LEN];
        pme[0..9].copy_from_slice(&[0x04, 0x00, 0x04, 0x00, 0x53, 0x00, 0x84, 0x00, 0x02]);
        pme[9] = 0x01; // enrolled
        pme[10] = 0x02; // calls off
        pme[11] = 0x02; // media off
        in_tx.send(pme.to_vec()).await.unwrap();
        tokio::task::yield_now().await;

        assert!(device.setting(&SettingId::HearingAid).is_some());

        // Enable HearingAid + MediaAssistCalls: writes 0x2C + 0x53 BEFORE 0x33=01 (300ms delay)
        device
            .set_setting_values(vec![
                (SettingId::HearingAid, Value::from(true)),
                (SettingId::MediaAssistCalls, Value::from(true)),
            ])
            .await
            .unwrap();

        let p1 = sent_rx.recv().await.unwrap();
        let p2 = sent_rx.recv().await.unwrap();
        let p3 = sent_rx.recv().await.unwrap();
        assert_eq!(p1, aap::set_hearing_aid(true));
        assert_eq!(p2.len(), PME_RECORD_LEN);
        assert_eq!(p2[10], 0x01);
        assert_eq!(p3, aap::set_hearing_assist(true));

        // Disable both: writes 0x33=02 FIRST, 300ms delay, then 0x2C=off + 0x53=off
        device
            .set_setting_values(vec![
                (SettingId::HearingAid, Value::from(false)),
                (SettingId::MediaAssistCalls, Value::from(false)),
            ])
            .await
            .unwrap();

        let d1 = sent_rx.recv().await.unwrap();
        let d2 = sent_rx.recv().await.unwrap();
        let d3 = sent_rx.recv().await.unwrap();
        assert_eq!(d1, aap::set_hearing_assist(false));
        assert_eq!(d2, aap::set_hearing_aid(false));
        assert_eq!(d3.len(), PME_RECORD_LEN);
        assert_eq!(d3[10], 0x02);
    }

    #[tokio::test(start_paused = true)]
    async fn conversation_awareness_speaking_and_2000ms_quiet_restore() {
        let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(64);
        let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
        let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());

        let responder_tx = in_tx.clone();
        tokio::spawn(async move {
            while let Some(pkt) = out_rx.recv().await {
                if pkt == aap::handshake() {
                    let _ = responder_tx
                        .send(vec![
                            0x04, 0x00, 0x04, 0x00, 0x1D, 0x00, 0, 0, 0, 0, 0, b'P', 0,
                        ])
                        .await;
                } else if pkt.len() >= 6 && pkt[4..6] == [0x0F, 0x00] {
                    let _ = responder_tx
                        .send(aap::set_conversation_awareness(true))
                        .await;
                }
            }
        });

        let backend = Arc::new(MockAapBackend::new(in_rx, out_tx));
        let device =
            AirPodsPro2Device::connect(backend, db, MacAddr6::nil(), AirPodsPro2Config::default())
                .await
                .unwrap();

        in_tx
            .send(vec![
                0x04, 0x00, 0x04, 0x00, 0x4B, 0x00, 0x02, 0x00, 0x01, 0x02,
            ])
            .await
            .unwrap();
        tokio::task::yield_now().await;
        assert_eq!(
            device.state().ca_state,
            ConversationAwarenessState::Speaking
        );

        in_tx
            .send(vec![
                0x04, 0x00, 0x04, 0x00, 0x4B, 0x00, 0x02, 0x00, 0x01, 0x05,
            ])
            .await
            .unwrap();
        tokio::task::yield_now().await;
        assert_eq!(
            device.state().ca_state,
            ConversationAwarenessState::Recovering
        );

        // After 2000ms with no end packet, restores to Inactive
        tokio::time::advance(Duration::from_millis(2100)).await;
        tokio::task::yield_now().await;
        assert_eq!(
            device.state().ca_state,
            ConversationAwarenessState::Inactive
        );
    }

    #[tokio::test(start_paused = true)]
    async fn demo_registry_roundtrips_all_categories_and_settings() {
        let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let registry = DemoAirPodsPro2Registry::new(db.clone());
        let devices = registry.devices().await.unwrap();
        assert_eq!(devices.len(), 1);

        let device = registry.connect(devices[0].mac_address).await.unwrap();
        assert_eq!(
            device.categories(),
            vec![
                CategoryId::SoundModes,
                CategoryId::General,
                CategoryId::ButtonConfiguration,
                CategoryId::HearingAssistance,
                CategoryId::DeviceInformation,
            ]
        );

        // Check read-only metadata and battery populated
        assert_eq!(
            Value::from(device.setting(&SettingId::ModelNumber).unwrap()),
            Value::from("A3048")
        );
        assert_eq!(
            Value::from(device.setting(&SettingId::IsChargingCase).unwrap()),
            Value::from("true")
        );

        // Exercise setting mutations
        device
            .set_setting_values(vec![
                (SettingId::ToneVolume, Value::from(45)),
                (
                    SettingId::PressSpeed,
                    Value::from(Into::<&'static str>::into(PressSpeed::Slower)),
                ),
                (
                    SettingId::HoldDuration,
                    Value::from(Into::<&'static str>::into(HoldDuration::Shortest)),
                ),
                (
                    SettingId::MicSide,
                    Value::from(Into::<&'static str>::into(MicSide::Right)),
                ),
                (SettingId::OneBudAnc, Value::from(true)),
                (SettingId::PersonalizedVolume, Value::from(false)),
            ])
            .await
            .unwrap();

        assert_eq!(
            Value::from(device.setting(&SettingId::ToneVolume).unwrap()),
            Value::from(45)
        );
        assert_eq!(
            Value::from(device.setting(&SettingId::MicSide).unwrap()),
            Value::from("Right")
        );
        let _ = PressSpeed::Slower;
        let _ = HoldDuration::Shortest;
    }

    #[tokio::test(start_paused = true)]
    async fn disconnect_and_call_audio_propagate_from_backend() {
        use crate::connection_backend::mock::aap::MockAapConnection;

        let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(64);
        let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
        let conn = Arc::new(MockAapConnection::new(in_rx, out_tx));
        let conn_clone = conn.clone();

        struct SingleConnBackend(Arc<MockAapConnection>);
        #[async_trait]
        impl AapBackend for SingleConnBackend {
            async fn devices(&self) -> connection::Result<HashSet<ConnectionDescriptor>> {
                Ok(HashSet::new())
            }
            async fn connect(
                &self,
                _mac_address: MacAddr6,
            ) -> connection::Result<Arc<dyn AapConnection + Send + Sync>> {
                Ok(self.0.clone())
            }
        }

        tokio::spawn(async move {
            while let Some(pkt) = out_rx.recv().await {
                if pkt == aap::handshake() {
                    let _ = in_tx
                        .send(vec![
                            0x04, 0x00, 0x04, 0x00, 0x1D, 0x00, 0, 0, 0, 0, 0, b'P', 0,
                        ])
                        .await;
                } else if pkt.len() >= 6 && pkt[4..6] == [0x0F, 0x00] {
                    let _ = in_tx
                        .send(aap::set_listening_mode(AapListeningMode::Adaptive))
                        .await;
                }
            }
        });

        let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let device = AirPodsPro2Device::connect(
            Arc::new(SingleConnBackend(conn_clone)),
            db,
            MacAddr6::nil(),
            AirPodsPro2Config::default(),
        )
        .await
        .unwrap();

        let mut status_rx = device.connection_status();
        assert_eq!(*status_rx.borrow_and_update(), ConnectionStatus::Connected);

        conn.set_call_audio_active(true);
        tokio::task::yield_now().await;
        assert!(device.state().call_audio_active);

        conn.set_connection_status(ConnectionStatus::Disconnected);
        status_rx.changed().await.unwrap();
        assert_eq!(
            *status_rx.borrow_and_update(),
            ConnectionStatus::Disconnected
        );
    }

    #[tokio::test(start_paused = true)]
    async fn inbound_0x09_during_initial_report_window_does_not_clobber_persisted_replay() {
        let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(64);
        let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
        let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let mac = MacAddr6::new(0x11, 0x22, 0x33, 0x44, 0x55, 0x66);

        // Host-persisted settings in SQLite
        db.upsert_device_setting(mac, "allow_off".to_owned(), "false".to_owned())
            .await
            .unwrap();
        db.upsert_device_setting(
            mac,
            "stem_modes".to_owned(),
            (StemModeMask::NOISE_CANCELING | StemModeMask::TRANSPARENCY)
                .bits()
                .to_string(),
        )
        .await
        .unwrap();
        db.upsert_device_setting(mac, "ear_detection".to_owned(), "false".to_owned())
            .await
            .unwrap();
        db.upsert_device_setting(mac, "mic_side".to_owned(), "Left".to_owned())
            .await
            .unwrap();

        let responder_tx = in_tx.clone();
        let (sent_tx, mut sent_rx) = mpsc::channel(64);
        tokio::spawn(async move {
            while let Some(pkt) = out_rx.recv().await {
                let _ = sent_tx.send(pkt.clone()).await;
                if pkt == aap::handshake() {
                    let _ = responder_tx
                        .send(vec![
                            0x04, 0x00, 0x04, 0x00, 0x1D, 0x00, 0, 0, 0, 0, 0, b'P', 0,
                        ])
                        .await;
                } else if pkt.len() >= 6 && pkt[4..6] == [0x0F, 0x00] {
                    // Pod sends stale/opposite values during initial_report_window:
                    // (0x34=01, 0x1A=0F, 0x0A=01, 0x01=01)
                    let _ = responder_tx.send(aap::set_allow_off(true)).await;
                    let _ = responder_tx
                        .send(aap::set_stem_modes(StemModeMask::all()))
                        .await;
                    let _ = responder_tx.send(aap::set_ear_detection(true)).await;
                    let _ = responder_tx.send(aap::set_mic_side(MicSide::Right)).await;
                }
            }
        });

        let backend = Arc::new(MockAapBackend::new(in_rx, out_tx));
        let device = AirPodsPro2Device::connect(backend, db, mac, AirPodsPro2Config::default())
            .await
            .unwrap();

        assert_eq!(device.state().phase, SessionPhase::InitialReportDone);

        let mut sent_0x09 = Vec::new();
        while let Ok(pkt) = sent_rx.try_recv() {
            if pkt.len() >= 8 && pkt[4..6] == [0x09, 0x00] {
                sent_0x09.push((pkt[6], pkt[7]));
            }
        }

        assert_eq!(
            sent_0x09,
            vec![(0x34, 0x02), (0x1A, 0x06), (0x0A, 0x02), (0x01, 0x02)]
        );

        let state = device.state();
        assert!(!state.allow_off);
        assert_eq!(
            state.stem_modes,
            StemModeMask::NOISE_CANCELING | StemModeMask::TRANSPARENCY
        );
        assert!(!state.ear_detection);
        assert_eq!(state.mic_side, MicSide::Left);
    }

    #[tokio::test(start_paused = true)]
    async fn demo_registry_persists_host_settings_across_reconnect() {
        let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let mac = DeviceModel::AppleAirPodsPro2.demo_mac_address();

        {
            let registry1 = DemoAirPodsPro2Registry::new(db.clone());
            let dev1 = registry1.connect(mac).await.unwrap();
            dev1.set_setting_values(vec![
                (SettingId::AllowOffListeningMode, Value::from(false)),
                (
                    SettingId::StemCycleModes,
                    Value::from(vec![
                        Cow::Borrowed("NoiseCanceling"),
                        Cow::Borrowed("Transparency"),
                    ]),
                ),
                (SettingId::EarDetection, Value::from(false)),
                (
                    SettingId::MicSide,
                    Value::from(Into::<&'static str>::into(MicSide::Left)),
                ),
            ])
            .await
            .unwrap();
        }
        tokio::task::yield_now().await;

        let registry2 = DemoAirPodsPro2Registry::new(db);
        let dev2 = registry2.connect(mac).await.unwrap();

        assert_eq!(
            Value::from(dev2.setting(&SettingId::AllowOffListeningMode).unwrap()),
            Value::from(false)
        );
        let stem_values = match dev2.setting(&SettingId::StemCycleModes).unwrap() {
            Setting::MultiSelect { values, .. } => values,
            other => panic!("expected MultiSelect, got {other:?}"),
        };
        assert_eq!(
            stem_values,
            vec![
                Cow::Borrowed("NoiseCanceling"),
                Cow::Borrowed("Transparency"),
            ]
        );
        assert_eq!(
            Value::from(dev2.setting(&SettingId::EarDetection).unwrap()),
            Value::from(false)
        );
        assert_eq!(
            Value::from(dev2.setting(&SettingId::MicSide).unwrap()),
            Value::from("Left")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn reattach_failure_immediately_retries_next_attempt_without_60s_wait() {
        struct FallibleMultiAttachBackend {
            outcomes: StdMutex<VecDeque<connection::Result<Arc<dyn AapConnection + Send + Sync>>>>,
            connect_calls: Arc<AtomicUsize>,
        }

        #[async_trait]
        impl AapBackend for FallibleMultiAttachBackend {
            async fn devices(&self) -> connection::Result<HashSet<ConnectionDescriptor>> {
                Ok(HashSet::new())
            }

            async fn connect(
                &self,
                _mac_address: MacAddr6,
            ) -> connection::Result<Arc<dyn AapConnection + Send + Sync>> {
                self.connect_calls.fetch_add(1, Ordering::SeqCst);
                self.outcomes.lock().unwrap().pop_front().unwrap()
            }
        }

        let (c0, in_tx0, mut out_rx0) = make_conn();
        let (c2, in_tx2, mut out_rx2) = make_conn();

        // c0 answers handshake but never sends 0x09
        tokio::spawn(async move {
            while let Some(pkt) = out_rx0.recv().await {
                if pkt == aap::handshake() {
                    let _ = in_tx0
                        .send(vec![
                            0x04, 0x00, 0x04, 0x00, 0x1D, 0x00, 0, 0, 0, 0, 0, b'A', 0,
                        ])
                        .await;
                }
            }
        });

        // c2 answers handshake and 0x0F with 0x09
        tokio::spawn(async move {
            while let Some(pkt) = out_rx2.recv().await {
                if pkt == aap::handshake() {
                    let _ = in_tx2
                        .send(vec![
                            0x04, 0x00, 0x04, 0x00, 0x1D, 0x00, 0, 0, 0, 0, 0, b'A', 0,
                        ])
                        .await;
                } else if pkt.len() >= 6 && pkt[4..6] == [0x0F, 0x00] {
                    let _ = in_tx2
                        .send(aap::set_listening_mode(AapListeningMode::Adaptive))
                        .await;
                }
            }
        });

        let connect_calls = Arc::new(AtomicUsize::new(0));
        let backend = Arc::new(FallibleMultiAttachBackend {
            outcomes: StdMutex::new(VecDeque::from([
                Ok(c0 as Arc<dyn AapConnection + Send + Sync>),
                Err(connection::Error::TimedOut { action: "connect" }),
                Ok(c2 as Arc<dyn AapConnection + Send + Sync>),
            ])),
            connect_calls: connect_calls.clone(),
        });

        let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let device =
            AirPodsPro2Device::connect(backend, db, MacAddr6::nil(), AirPodsPro2Config::default())
                .await
                .unwrap();

        assert_eq!(connect_calls.load(Ordering::SeqCst), 1);

        // Advance past the single 60s initial_report_timeout + initial_report_window (well under 120s).
        // When reattach attempt #1 fails, attempt #2 must run immediately (total 3 connect calls).
        tokio::time::sleep(Duration::from_secs(65)).await;

        assert_eq!(connect_calls.load(Ordering::SeqCst), 3);
        assert_eq!(device.state().phase, SessionPhase::InitialReportDone);
    }

    struct EchoingFailConn {
        inbound_tx: mpsc::Sender<Vec<u8>>,
        inbound_rx: StdMutex<Option<mpsc::Receiver<Vec<u8>>>>,
        sent_tx: mpsc::Sender<Vec<u8>>,
        status_tx: watch::Sender<ConnectionStatus>,
        initial_listening_mode: AapListeningMode,
        initial_allow_off: bool,
        initial_stem_modes: StemModeMask,
        suppress_0x0d_echo: std::sync::atomic::AtomicBool,
        delay_on_allow_off_false: bool,
        fail_sub_opcodes: StdMutex<VecDeque<u8>>,
    }

    impl EchoingFailConn {
        fn new(
            initial_listening_mode: AapListeningMode,
            initial_allow_off: bool,
            initial_stem_modes: StemModeMask,
            delay_on_allow_off_false: bool,
        ) -> (Arc<Self>, mpsc::Receiver<Vec<u8>>) {
            let (inbound_tx, inbound_rx) = mpsc::channel(128);
            let (sent_tx, sent_rx) = mpsc::channel(128);
            let (status_tx, _) = watch::channel(ConnectionStatus::Connected);
            (
                Arc::new(Self {
                    inbound_tx,
                    inbound_rx: StdMutex::new(Some(inbound_rx)),
                    sent_tx,
                    status_tx,
                    initial_listening_mode,
                    initial_allow_off,
                    initial_stem_modes,
                    suppress_0x0d_echo: std::sync::atomic::AtomicBool::new(false),
                    delay_on_allow_off_false,
                    fail_sub_opcodes: StdMutex::new(VecDeque::new()),
                }),
                sent_rx,
            )
        }
    }

    #[async_trait]
    impl AapConnection for EchoingFailConn {
        async fn write(&self, data: &[u8]) -> connection::Result<()> {
            if data == aap::handshake() {
                let _ = self
                    .inbound_tx
                    .send(vec![
                        0x04, 0x00, 0x04, 0x00, 0x1D, 0x00, 0, 0, 0, 0, 0, b'P', 0,
                    ])
                    .await;
                return Ok(());
            }
            if data.len() >= 6 && data[4..6] == [0x0F, 0x00] {
                let _ = self
                    .inbound_tx
                    .send(aap::set_listening_mode(self.initial_listening_mode))
                    .await;
                let _ = self
                    .inbound_tx
                    .send(aap::set_allow_off(self.initial_allow_off))
                    .await;
                let _ = self
                    .inbound_tx
                    .send(aap::set_stem_modes(self.initial_stem_modes))
                    .await;
                return Ok(());
            }
            if data.len() >= 8 && data[4..6] == [0x09, 0x00] {
                let should_fail = {
                    let mut queue = self.fail_sub_opcodes.lock().unwrap();
                    if queue.front().copied() == Some(data[6]) {
                        queue.pop_front();
                        true
                    } else {
                        false
                    }
                };
                if should_fail {
                    return Err(connection::Error::WriteError {
                        source: None,
                        location: std::panic::Location::caller(),
                    });
                }

                let _ = self.sent_tx.send(data.to_vec()).await;
                if !(data[6] == 0x0D && self.suppress_0x0d_echo.load(Ordering::SeqCst)) {
                    let _ = self.inbound_tx.send(data.to_vec()).await;
                }
                if self.delay_on_allow_off_false && data == aap::set_allow_off(false) {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
            Ok(())
        }

        fn read_channel(&self) -> mpsc::Receiver<Vec<u8>> {
            self.inbound_rx.lock().unwrap().take().unwrap()
        }

        fn connection_status(&self) -> watch::Receiver<ConnectionStatus> {
            self.status_tx.subscribe()
        }
    }

    struct SingleEchoBackend(Arc<EchoingFailConn>);

    #[async_trait]
    impl AapBackend for SingleEchoBackend {
        async fn devices(&self) -> connection::Result<HashSet<ConnectionDescriptor>> {
            Ok(HashSet::new())
        }

        async fn connect(
            &self,
            _mac_address: MacAddr6,
        ) -> connection::Result<Arc<dyn AapConnection + Send + Sync>> {
            Ok(self.0.clone())
        }
    }

    struct TempDbDir {
        dir: std::path::PathBuf,
    }

    impl TempDbDir {
        fn new() -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "openscq30_apple_test_{}_{}_{}",
                std::process::id(),
                nanos,
                seq
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self { dir }
        }

        fn db_path(&self) -> std::path::PathBuf {
            self.dir.join("test.sqlite")
        }
    }

    impl Drop for TempDbDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn allow_off_false_transmits_transparency_despite_immediate_0x34_echo_and_stays_transparency_on_reenable()
     {
        // Sub-case A: Confirmed latest_device_listening_mode == Some(Off) with 0x34=0x02 echo + 1ms yield
        let (conn_a, mut sent_rx_a) = EchoingFailConn::new(
            AapListeningMode::Off,
            true,
            StemModeMask::OFF | StemModeMask::NOISE_CANCELING,
            true,
        );
        let db_a = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let device_a = AirPodsPro2Device::connect(
            Arc::new(SingleEchoBackend(conn_a)),
            db_a,
            MacAddr6::nil(),
            AirPodsPro2Config::default(),
        )
        .await
        .unwrap();

        assert_eq!(
            device_a.state().latest_device_listening_mode,
            Some(AapListeningMode::Off)
        );
        while sent_rx_a.try_recv().is_ok() {}

        device_a
            .set_setting_values(vec![(SettingId::AllowOffListeningMode, Value::from(false))])
            .await
            .unwrap();

        let mut sent_a = Vec::new();
        while let Ok(pkt) = sent_rx_a.try_recv() {
            sent_a.push(pkt);
        }
        assert!(sent_a.contains(&vec![
            0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x34, 0x02, 0x00, 0x00, 0x00
        ]));
        assert!(sent_a.contains(&vec![
            0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x1A, 0x06, 0x00, 0x00, 0x00
        ]));
        assert!(sent_a.contains(&vec![
            0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x0D, 0x03, 0x00, 0x00, 0x00
        ]));

        tokio::task::yield_now().await;
        assert_eq!(
            device_a.state().latest_device_listening_mode,
            Some(AapListeningMode::Transparency)
        );

        device_a
            .set_setting_values(vec![(SettingId::AllowOffListeningMode, Value::from(true))])
            .await
            .unwrap();
        assert_eq!(
            Value::from(device_a.setting(&SettingId::ListeningMode).unwrap()),
            Value::from("Transparency")
        );

        tokio::time::advance(Duration::from_millis(3100)).await;
        tokio::task::yield_now().await;
        assert_eq!(
            Value::from(device_a.setting(&SettingId::ListeningMode).unwrap()),
            Value::from("Transparency")
        );
        assert_eq!(
            device_a.state().latest_device_listening_mode,
            Some(AapListeningMode::Transparency)
        );

        // Sub-case B: Active optimistic Off (no 0x0D=0x01 echo yet) + immediate 0x34=0x02 echo
        let (conn_b, mut sent_rx_b) = EchoingFailConn::new(
            AapListeningMode::Adaptive,
            true,
            StemModeMask::OFF | StemModeMask::NOISE_CANCELING,
            true,
        );
        let db_b = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let device_b = AirPodsPro2Device::connect(
            Arc::new(SingleEchoBackend(conn_b.clone())),
            db_b,
            MacAddr6::nil(),
            AirPodsPro2Config::default(),
        )
        .await
        .unwrap();

        conn_b.suppress_0x0d_echo.store(true, Ordering::SeqCst);
        device_b
            .set_setting_values(vec![(SettingId::ListeningMode, Value::from("Off"))])
            .await
            .unwrap();
        assert_eq!(
            device_b.state().latest_device_listening_mode,
            Some(AapListeningMode::Adaptive)
        );
        assert_eq!(
            device_b
                .state()
                .optimistic_listening_modes
                .back()
                .map(|p| p.mode),
            Some(AapListeningMode::Off)
        );

        conn_b.suppress_0x0d_echo.store(false, Ordering::SeqCst);
        while sent_rx_b.try_recv().is_ok() {}

        device_b
            .set_setting_values(vec![(SettingId::AllowOffListeningMode, Value::from(false))])
            .await
            .unwrap();

        let mut sent_b = Vec::new();
        while let Ok(pkt) = sent_rx_b.try_recv() {
            sent_b.push(pkt);
        }
        assert!(sent_b.contains(&vec![
            0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x0D, 0x03, 0x00, 0x00, 0x00
        ]));

        tokio::task::yield_now().await;
        assert_eq!(
            device_b.state().latest_device_listening_mode,
            Some(AapListeningMode::Transparency)
        );

        // Sub-case C: allow_off is already false while latest_device_listening_mode == Some(Off)
        let (conn_c, mut sent_rx_c) = EchoingFailConn::new(
            AapListeningMode::Off,
            false,
            StemModeMask::NOISE_CANCELING | StemModeMask::TRANSPARENCY,
            true,
        );
        let db_c = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let device_c = AirPodsPro2Device::connect(
            Arc::new(SingleEchoBackend(conn_c)),
            db_c,
            MacAddr6::nil(),
            AirPodsPro2Config::default(),
        )
        .await
        .unwrap();

        assert!(!device_c.state().allow_off);
        assert_eq!(
            device_c.state().latest_device_listening_mode,
            Some(AapListeningMode::Off)
        );
        while sent_rx_c.try_recv().is_ok() {}

        device_c
            .set_setting_values(vec![(SettingId::AllowOffListeningMode, Value::from(false))])
            .await
            .unwrap();

        let mut sent_c = Vec::new();
        while let Ok(pkt) = sent_rx_c.try_recv() {
            sent_c.push(pkt);
        }
        assert!(sent_c.contains(&vec![
            0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x0D, 0x03, 0x00, 0x00, 0x00
        ]));
        tokio::task::yield_now().await;
        assert_eq!(
            device_c.state().latest_device_listening_mode,
            Some(AapListeningMode::Transparency)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn allow_off_and_stem_modes_rollback_and_consistency_on_send_and_persist_failures() {
        let mac = MacAddr6::new(0x22, 0x33, 0x44, 0x55, 0x66, 0x77);
        let initial_stem = StemModeMask::OFF | StemModeMask::NOISE_CANCELING;

        // 3a: send_packet(0x1A) fails during AllowOffListeningMode = false
        {
            let (conn, _sent_rx) =
                EchoingFailConn::new(AapListeningMode::Off, true, initial_stem, false);
            let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
            let device = AirPodsPro2Device::connect(
                Arc::new(SingleEchoBackend(conn.clone())),
                db.clone(),
                mac,
                AirPodsPro2Config::default(),
            )
            .await
            .unwrap();

            device
                .set_setting_values(vec![(
                    SettingId::StemCycleModes,
                    Value::from(vec![Cow::Borrowed("Off"), Cow::Borrowed("NoiseCanceling")]),
                )])
                .await
                .unwrap();

            conn.fail_sub_opcodes.lock().unwrap().push_back(0x1A);
            let err = device
                .set_setting_values(vec![(SettingId::AllowOffListeningMode, Value::from(false))])
                .await
                .unwrap_err();
            assert!(matches!(err, device::Error::ConnectionError { .. }));

            let s = device.state();
            let db_settings = db.fetch_device_settings(mac).await.unwrap();
            assert!(!s.allow_off);
            assert_eq!(s.persisted.allow_off, Some(false));
            assert_eq!(
                db_settings.get("allow_off").map(String::as_str),
                Some("false")
            );
            assert_eq!(s.stem_modes, initial_stem);
            assert_eq!(s.persisted.stem_modes, Some(initial_stem));
            assert_eq!(
                db_settings.get("stem_modes").map(String::as_str),
                Some(initial_stem.bits().to_string().as_str())
            );
            assert!(s.optimistic_listening_modes.is_empty());
        }

        // 3b: persist_setting(KEY_STEM_MODES) fails during AllowOffListeningMode = false
        {
            let temp_dir = TempDbDir::new();
            let db_path = temp_dir.db_path();
            let db = Arc::new(OpenSCQ30Database::new_file(db_path.clone()).await.unwrap());
            let (conn, _sent_rx) =
                EchoingFailConn::new(AapListeningMode::Off, true, initial_stem, false);
            let device = AirPodsPro2Device::connect(
                Arc::new(SingleEchoBackend(conn)),
                db.clone(),
                mac,
                AirPodsPro2Config::default(),
            )
            .await
            .unwrap();

            device
                .set_setting_values(vec![(
                    SettingId::StemCycleModes,
                    Value::from(vec![Cow::Borrowed("Off"), Cow::Borrowed("NoiseCanceling")]),
                )])
                .await
                .unwrap();

            {
                let raw = rusqlite::Connection::open(&db_path).unwrap();
                raw.execute_batch(
                    "CREATE TRIGGER fail_stem_modes_insert BEFORE INSERT ON device_settings
                     WHEN NEW.setting_key = 'stem_modes'
                     BEGIN SELECT RAISE(ABORT, 'injected stem_modes persist failure'); END;
                     CREATE TRIGGER fail_stem_modes_update BEFORE UPDATE ON device_settings
                     WHEN NEW.setting_key = 'stem_modes'
                     BEGIN SELECT RAISE(ABORT, 'injected stem_modes persist failure'); END;",
                )
                .unwrap();
            }

            let err = device
                .set_setting_values(vec![(SettingId::AllowOffListeningMode, Value::from(false))])
                .await
                .unwrap_err();
            assert!(matches!(err, device::Error::StorageError { .. }));

            let s = device.state();
            let db_settings = db.fetch_device_settings(mac).await.unwrap();
            assert!(!s.allow_off);
            assert_eq!(
                db_settings.get("allow_off").map(String::as_str),
                Some("false")
            );
            assert_ne!(
                s.persisted.stem_modes,
                Some(StemModeMask::NOISE_CANCELING | StemModeMask::TRANSPARENCY)
            );
            assert_eq!(s.persisted.stem_modes, Some(initial_stem));
            assert_eq!(
                db_settings.get("stem_modes").map(String::as_str),
                Some(initial_stem.bits().to_string().as_str())
            );
            assert!(s.optimistic_listening_modes.is_empty());
        }

        // 3c: send_packet(0x0D = Transparency) fails during AllowOffListeningMode = false
        {
            let (conn, _sent_rx) =
                EchoingFailConn::new(AapListeningMode::Off, true, initial_stem, false);
            let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
            let device = AirPodsPro2Device::connect(
                Arc::new(SingleEchoBackend(conn.clone())),
                db.clone(),
                mac,
                AirPodsPro2Config::default(),
            )
            .await
            .unwrap();

            conn.fail_sub_opcodes.lock().unwrap().push_back(0x0D);
            let err = device
                .set_setting_values(vec![(SettingId::AllowOffListeningMode, Value::from(false))])
                .await
                .unwrap_err();
            assert!(matches!(err, device::Error::ConnectionError { .. }));

            let s = device.state();
            let db_settings = db.fetch_device_settings(mac).await.unwrap();
            let expected_stem = StemModeMask::NOISE_CANCELING | StemModeMask::TRANSPARENCY;
            assert!(s.optimistic_listening_modes.is_empty());
            assert_eq!(s.latest_device_listening_mode, Some(AapListeningMode::Off));
            assert!(!s.allow_off);
            assert_eq!(s.stem_modes, expected_stem);
            assert_eq!(
                db_settings.get("allow_off").map(String::as_str),
                Some("false")
            );
            assert_eq!(
                db_settings.get("stem_modes").map(String::as_str),
                Some(expected_stem.bits().to_string().as_str())
            );
        }

        // 3d: StemCycleModes fails on send_packet(0x1A) and on persist_setting(KEY_STEM_MODES)
        {
            let temp_dir = TempDbDir::new();
            let db_path = temp_dir.db_path();
            let db = Arc::new(OpenSCQ30Database::new_file(db_path.clone()).await.unwrap());
            let (conn, _sent_rx) =
                EchoingFailConn::new(AapListeningMode::Adaptive, true, initial_stem, false);
            let device = AirPodsPro2Device::connect(
                Arc::new(SingleEchoBackend(conn.clone())),
                db.clone(),
                mac,
                AirPodsPro2Config::default(),
            )
            .await
            .unwrap();

            device
                .set_setting_values(vec![(
                    SettingId::StemCycleModes,
                    Value::from(vec![Cow::Borrowed("Off"), Cow::Borrowed("NoiseCanceling")]),
                )])
                .await
                .unwrap();

            conn.fail_sub_opcodes.lock().unwrap().push_back(0x1A);
            let err1 = device
                .set_setting_values(vec![(
                    SettingId::StemCycleModes,
                    Value::from(vec![
                        Cow::Borrowed("Transparency"),
                        Cow::Borrowed("Adaptive"),
                    ]),
                )])
                .await
                .unwrap_err();
            assert!(matches!(err1, device::Error::ConnectionError { .. }));
            assert_eq!(device.state().stem_modes, initial_stem);
            assert_eq!(device.state().persisted.stem_modes, Some(initial_stem));

            {
                let raw = rusqlite::Connection::open(&db_path).unwrap();
                raw.execute_batch(
                    "CREATE TRIGGER fail_stem_modes_insert BEFORE INSERT ON device_settings
                     WHEN NEW.setting_key = 'stem_modes'
                     BEGIN SELECT RAISE(ABORT, 'injected stem_modes persist failure'); END;
                     CREATE TRIGGER fail_stem_modes_update BEFORE UPDATE ON device_settings
                     WHEN NEW.setting_key = 'stem_modes'
                     BEGIN SELECT RAISE(ABORT, 'injected stem_modes persist failure'); END;",
                )
                .unwrap();
            }
            let err2 = device
                .set_setting_values(vec![(
                    SettingId::StemCycleModes,
                    Value::from(vec![
                        Cow::Borrowed("Transparency"),
                        Cow::Borrowed("Adaptive"),
                    ]),
                )])
                .await
                .unwrap_err();
            assert!(matches!(err2, device::Error::StorageError { .. }));
            assert_eq!(device.state().persisted.stem_modes, Some(initial_stem));
            assert_eq!(
                db.fetch_device_settings(mac)
                    .await
                    .unwrap()
                    .get("stem_modes")
                    .map(String::as_str),
                Some(initial_stem.bits().to_string().as_str())
            );
        }

        // 3e: send_packet(0x34) fails and persist_setting(KEY_ALLOW_OFF) fails
        {
            let temp_dir = TempDbDir::new();
            let db_path = temp_dir.db_path();
            let db = Arc::new(OpenSCQ30Database::new_file(db_path.clone()).await.unwrap());
            let (conn, mut sent_rx) =
                EchoingFailConn::new(AapListeningMode::Off, true, initial_stem, false);
            let device = AirPodsPro2Device::connect(
                Arc::new(SingleEchoBackend(conn.clone())),
                db.clone(),
                mac,
                AirPodsPro2Config::default(),
            )
            .await
            .unwrap();

            device
                .set_setting_values(vec![
                    (SettingId::AllowOffListeningMode, Value::from(true)),
                    (
                        SettingId::StemCycleModes,
                        Value::from(vec![Cow::Borrowed("Off"), Cow::Borrowed("NoiseCanceling")]),
                    ),
                ])
                .await
                .unwrap();
            while sent_rx.try_recv().is_ok() {}

            conn.fail_sub_opcodes.lock().unwrap().push_back(0x34);
            let err1 = device
                .set_setting_values(vec![(SettingId::AllowOffListeningMode, Value::from(false))])
                .await
                .unwrap_err();
            assert!(matches!(err1, device::Error::ConnectionError { .. }));
            assert_eq!(sent_rx.try_recv(), Err(mpsc::error::TryRecvError::Empty));
            assert!(device.state().allow_off);
            assert_eq!(device.state().persisted.allow_off, Some(true));
            assert_eq!(device.state().stem_modes, initial_stem);

            {
                let raw = rusqlite::Connection::open(&db_path).unwrap();
                raw.execute_batch(
                    "CREATE TRIGGER fail_allow_off_insert BEFORE INSERT ON device_settings
                     WHEN NEW.setting_key = 'allow_off'
                     BEGIN SELECT RAISE(ABORT, 'injected allow_off persist failure'); END;
                     CREATE TRIGGER fail_allow_off_update BEFORE UPDATE ON device_settings
                     WHEN NEW.setting_key = 'allow_off'
                     BEGIN SELECT RAISE(ABORT, 'injected allow_off persist failure'); END;",
                )
                .unwrap();
            }
            let err2 = device
                .set_setting_values(vec![(SettingId::AllowOffListeningMode, Value::from(false))])
                .await
                .unwrap_err();
            assert!(matches!(err2, device::Error::StorageError { .. }));

            // Only 0x34 was sent; neither 0x1A nor 0x0D was sent
            let mut post_sent = Vec::new();
            while let Ok(pkt) = sent_rx.try_recv() {
                post_sent.push(pkt);
            }
            assert_eq!(post_sent, vec![aap::set_allow_off(false)]);
            let db_settings = db.fetch_device_settings(mac).await.unwrap();
            assert_eq!(
                db_settings.get("allow_off").map(String::as_str),
                Some("true")
            );
            assert_eq!(
                db_settings.get("stem_modes").map(String::as_str),
                Some(initial_stem.bits().to_string().as_str())
            );
            assert_eq!(device.state().persisted.allow_off, Some(true));
            assert_eq!(device.state().persisted.stem_modes, Some(initial_stem));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn watch_for_changes_fires_when_optimistic_listening_mode_expires() {
        let (conn, _sent_rx) = EchoingFailConn::new(
            AapListeningMode::NoiseCanceling,
            true,
            StemModeMask::NOISE_CANCELING | StemModeMask::TRANSPARENCY | StemModeMask::ADAPTIVE,
            false,
        );
        let db = Arc::new(OpenSCQ30Database::new_in_memory().await.unwrap());
        let device = AirPodsPro2Device::connect(
            Arc::new(SingleEchoBackend(conn.clone())),
            db,
            MacAddr6::nil(),
            AirPodsPro2Config::default(),
        )
        .await
        .unwrap();

        let mut changes = device.watch_for_changes();
        let _ = changes.borrow_and_update();

        conn.suppress_0x0d_echo.store(true, Ordering::SeqCst);
        device
            .set_setting_values(vec![(SettingId::ListeningMode, Value::from("Adaptive"))])
            .await
            .unwrap();
        changes.changed().await.unwrap();
        assert_eq!(
            Value::from(device.setting(&SettingId::ListeningMode).unwrap()),
            Value::from("Adaptive")
        );

        tokio::time::advance(Duration::from_millis(3100)).await;
        tokio::task::yield_now().await;
        assert!(changes.has_changed().unwrap());
        assert_eq!(
            Value::from(device.setting(&SettingId::ListeningMode).unwrap()),
            Value::from("NoiseCanceling")
        );
    }
}
