use std::{panic::Location, sync::Arc, time::Duration};

use async_trait::async_trait;
use macaddr::MacAddr6;
use thiserror::Error;
use tokio::sync::{Mutex, mpsc, watch};
use tracing::{debug, warn};

use crate::{
    api::connection::{self, AapConnection, ConnectionStatus},
    util::AbortOnDropHandle,
};

pub const WPODS_AAP_PIPE_NAME: &str = r"\\.\pipe\WPods.Aap";
pub const MAX_AAP_PACKET_LEN: usize = 1691;
pub const PIPE_READ_BUFFER_LEN: usize = 4096;

const OPEN_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum WPodsFrameError {
    #[error("invalid WPods.Aap frame: {0}")]
    InvalidFrame(&'static str),
    #[error("outbound AAP packet length {len} exceeds valid range 1..={MAX_AAP_PACKET_LEN}")]
    InvalidOutboundLength { len: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WPodsInboundMessage {
    AapPacket(Vec<u8>),
    ChannelClosed(i32),
    CallAudioActive(bool),
}

/// Encodes the `"AP"` + little-endian `u64` MAC address open request for `\\.\pipe\WPods.Aap`.
pub fn encode_open_request(mac_address: MacAddr6) -> [u8; 10] {
    let bytes = mac_address.into_array();
    let mac_u64 = u64::from_be_bytes([
        0, 0, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5],
    ]);
    let le_mac = mac_u64.to_le_bytes();
    [
        b'A', b'P', le_mac[0], le_mac[1], le_mac[2], le_mac[3], le_mac[4], le_mac[5], le_mac[6],
        le_mac[7],
    ]
}

/// Decodes the initial `0x02` + `i32` LE status reply from `\\.\pipe\WPods.Aap`.
pub fn decode_open_response(frame: &[u8]) -> Result<i32, WPodsFrameError> {
    if frame.len() != 5 || frame[0] != 0x02 {
        return Err(WPodsFrameError::InvalidFrame(
            "expected 5-byte 0x02 + i32 LE open response",
        ));
    }
    Ok(i32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]))
}

/// Validates an outbound raw AAP packet before writing to `\\.\pipe\WPods.Aap`.
pub fn validate_outbound_packet(data: &[u8]) -> Result<(), WPodsFrameError> {
    if data.is_empty() || data.len() > MAX_AAP_PACKET_LEN {
        return Err(WPodsFrameError::InvalidOutboundLength { len: data.len() });
    }
    Ok(())
}

/// Demuxes an inbound `\\.\pipe\WPods.Aap` message frame (`0x00` packet, `0x01` close, `0x03` HFP).
pub fn decode_inbound_message(frame: &[u8]) -> Result<WPodsInboundMessage, WPodsFrameError> {
    match frame {
        [0x00, payload @ ..] if !payload.is_empty() => {
            Ok(WPodsInboundMessage::AapPacket(payload.to_vec()))
        }
        [0x01, b0, b1, b2, b3] => Ok(WPodsInboundMessage::ChannelClosed(i32::from_le_bytes([
            *b0, *b1, *b2, *b3,
        ]))),
        [0x03, flag] => Ok(WPodsInboundMessage::CallAudioActive(*flag != 0)),
        _ => Err(WPodsFrameError::InvalidFrame(
            "unrecognized or malformed WPods.Aap inbound frame",
        )),
    }
}

/// Maps Win32 status codes returned in `WPods.Aap` `0x02` / `0x01` frames to clear descriptions.
pub fn describe_win32_open_error(code: i32) -> String {
    match code {
        1167 => "AirPods Bluetooth ACL link is not connected".to_owned(),
        170 => "WPods AAP channel is busy".to_owned(),
        1225 => "L2CAP PSM 0x1001 connection refused by AirPods".to_owned(),
        121 => "L2CAP connection timed out".to_owned(),
        _ => format!("Win32 error {code}"),
    }
}

#[async_trait]
pub trait MessagePipe: Send + Sync {
    async fn read_message(&self) -> std::io::Result<Option<Vec<u8>>>;
    async fn write_message(&self, msg: &[u8]) -> std::io::Result<()>;
}

pub struct WPodsAapConnection {
    pipe: Arc<dyn MessagePipe>,
    write_lock: Mutex<()>,
    read_channel: std::sync::Mutex<Option<mpsc::Receiver<Vec<u8>>>>,
    connection_status: watch::Sender<ConnectionStatus>,
    call_audio_active: watch::Sender<bool>,
    _reader_handle: AbortOnDropHandle<()>,
}

impl WPodsAapConnection {
    pub async fn open_with_pipe(
        pipe: Arc<dyn MessagePipe>,
        mac_address: MacAddr6,
    ) -> connection::Result<Self> {
        let open_req = encode_open_request(mac_address);
        pipe.write_message(&open_req)
            .await
            .map_err(|err| connection::Error::WriteError {
                source: Some(Box::new(err)),
                location: Location::caller(),
            })?;

        let reply = tokio::time::timeout(OPEN_HANDSHAKE_TIMEOUT, pipe.read_message())
            .await
            .map_err(|_| connection::Error::TimedOut {
                action: "open WPods AAP pipe",
            })?
            .map_err(|err| connection::Error::Other {
                source: Box::new(err),
                location: Location::caller(),
            })?;

        let frame = reply.ok_or_else(|| connection::Error::ServiceUnavailable {
            reason: "WPods.Aap pipe closed during open handshake".to_owned(),
            source: None,
            location: Location::caller(),
        })?;

        let status_code = decode_open_response(&frame).map_err(|err| connection::Error::Other {
            source: Box::new(err),
            location: Location::caller(),
        })?;

        if status_code != 0 {
            return Err(connection::Error::TransportError {
                code: status_code,
                message: describe_win32_open_error(status_code),
                location: Location::caller(),
            });
        }

        let (packet_tx, packet_rx) = mpsc::channel(128);
        let (status_tx, _) = watch::channel(ConnectionStatus::Connected);
        let (call_audio_tx, _) = watch::channel(false);

        let reader_pipe = pipe.clone();
        let reader_status_tx = status_tx.clone();
        let reader_call_audio_tx = call_audio_tx.clone();
        let reader_handle = AbortOnDropHandle::new(tokio::spawn(async move {
            loop {
                match reader_pipe.read_message().await {
                    Ok(Some(frame)) => match decode_inbound_message(&frame) {
                        Ok(WPodsInboundMessage::AapPacket(bytes)) => {
                            if packet_tx.send(bytes).await.is_err() {
                                break;
                            }
                        }
                        Ok(WPodsInboundMessage::ChannelClosed(code)) => {
                            debug!(
                                "WPods.Aap channel closed with status {code} ({})",
                                describe_win32_open_error(code)
                            );
                            break;
                        }
                        Ok(WPodsInboundMessage::CallAudioActive(active)) => {
                            reader_call_audio_tx.send_replace(active);
                        }
                        Err(err) => {
                            warn!("ignoring malformed WPods.Aap inbound frame: {err}");
                        }
                    },
                    Ok(None) => {
                        debug!("WPods.Aap pipe reached EOF");
                        break;
                    }
                    Err(err) => {
                        warn!("WPods.Aap pipe read error: {err}");
                        break;
                    }
                }
            }
            reader_status_tx.send_replace(ConnectionStatus::Disconnected);
        }));

        Ok(Self {
            pipe,
            write_lock: Mutex::new(()),
            read_channel: std::sync::Mutex::new(Some(packet_rx)),
            connection_status: status_tx,
            call_audio_active: call_audio_tx,
            _reader_handle: reader_handle,
        })
    }
}

#[async_trait]
impl AapConnection for WPodsAapConnection {
    async fn write(&self, data: &[u8]) -> connection::Result<()> {
        validate_outbound_packet(data).map_err(|err| connection::Error::WriteError {
            source: Some(Box::new(err)),
            location: Location::caller(),
        })?;
        let _guard = self.write_lock.lock().await;
        self.pipe
            .write_message(data)
            .await
            .map_err(|err| connection::Error::WriteError {
                source: Some(Box::new(err)),
                location: Location::caller(),
            })
    }

    fn read_channel(&self) -> mpsc::Receiver<Vec<u8>> {
        self.read_channel
            .lock()
            .unwrap()
            .take()
            .expect("WPodsAapConnection::read_channel should only be called once")
    }

    fn connection_status(&self) -> watch::Receiver<ConnectionStatus> {
        self.connection_status.subscribe()
    }

    fn call_audio_active(&self) -> Option<watch::Receiver<bool>> {
        Some(self.call_audio_active.subscribe())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    struct MockMessagePipe {
        inbound: tokio::sync::Mutex<mpsc::Receiver<std::io::Result<Option<Vec<u8>>>>>,
        outbound: mpsc::Sender<Vec<u8>>,
    }

    impl MockMessagePipe {
        fn new() -> (
            Arc<Self>,
            mpsc::Sender<std::io::Result<Option<Vec<u8>>>>,
            mpsc::Receiver<Vec<u8>>,
        ) {
            let (in_tx, in_rx) = mpsc::channel(32);
            let (out_tx, out_rx) = mpsc::channel(32);
            (
                Arc::new(Self {
                    inbound: tokio::sync::Mutex::new(in_rx),
                    outbound: out_tx,
                }),
                in_tx,
                out_rx,
            )
        }
    }

    #[async_trait]
    impl MessagePipe for MockMessagePipe {
        async fn read_message(&self) -> std::io::Result<Option<Vec<u8>>> {
            let mut rx = self.inbound.lock().await;
            match rx.recv().await {
                Some(item) => item,
                None => Ok(None),
            }
        }

        async fn write_message(&self, msg: &[u8]) -> std::io::Result<()> {
            self.outbound
                .send(msg.to_vec())
                .await
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed"))
        }
    }

    #[test]
    fn encode_open_request_matches_wpods_le_mac_layout() {
        let mac = MacAddr6::from_str("01:23:45:67:89:AB").unwrap();
        assert_eq!(
            encode_open_request(mac),
            [0x41, 0x50, 0xAB, 0x89, 0x67, 0x45, 0x23, 0x01, 0x00, 0x00]
        );
    }

    #[test]
    fn decode_open_response_parses_success_and_errors() {
        assert_eq!(decode_open_response(&[0x02, 0, 0, 0, 0]), Ok(0));
        let err_1167 = 1167_i32.to_le_bytes();
        assert_eq!(
            decode_open_response(&[0x02, err_1167[0], err_1167[1], err_1167[2], err_1167[3]]),
            Ok(1167)
        );
        assert!(matches!(
            decode_open_response(&[0x00, 0, 0, 0, 0]),
            Err(WPodsFrameError::InvalidFrame(_))
        ));
        assert!(matches!(
            decode_open_response(&[0x02, 0, 0, 0]),
            Err(WPodsFrameError::InvalidFrame(_))
        ));
    }

    #[test]
    fn validate_outbound_packet_enforces_bounds() {
        assert_eq!(
            validate_outbound_packet(&[]),
            Err(WPodsFrameError::InvalidOutboundLength { len: 0 })
        );
        assert_eq!(validate_outbound_packet(&[0x01]), Ok(()));
        assert_eq!(
            validate_outbound_packet(&vec![0xAA; MAX_AAP_PACKET_LEN]),
            Ok(())
        );
        assert_eq!(
            validate_outbound_packet(&vec![0xAA; MAX_AAP_PACKET_LEN + 1]),
            Err(WPodsFrameError::InvalidOutboundLength {
                len: MAX_AAP_PACKET_LEN + 1
            })
        );
    }

    #[test]
    fn decode_inbound_message_demuxes_all_frame_types() {
        assert_eq!(
            decode_inbound_message(&[0x00, 0x04, 0x00, 0x04, 0x00]),
            Ok(WPodsInboundMessage::AapPacket(vec![0x04, 0x00, 0x04, 0x00]))
        );
        let code = 1225_i32.to_le_bytes();
        assert_eq!(
            decode_inbound_message(&[0x01, code[0], code[1], code[2], code[3]]),
            Ok(WPodsInboundMessage::ChannelClosed(1225))
        );
        assert_eq!(
            decode_inbound_message(&[0x03, 1]),
            Ok(WPodsInboundMessage::CallAudioActive(true))
        );
        assert_eq!(
            decode_inbound_message(&[0x03, 0]),
            Ok(WPodsInboundMessage::CallAudioActive(false))
        );
        assert!(matches!(
            decode_inbound_message(&[0x00]),
            Err(WPodsFrameError::InvalidFrame(_))
        ));
        assert!(matches!(
            decode_inbound_message(&[0x01, 0, 0]),
            Err(WPodsFrameError::InvalidFrame(_))
        ));
        assert!(matches!(
            decode_inbound_message(&[0x99, 0]),
            Err(WPodsFrameError::InvalidFrame(_))
        ));
    }

    #[test]
    fn describe_win32_open_error_maps_known_codes() {
        assert!(describe_win32_open_error(1167).contains("ACL"));
        assert!(describe_win32_open_error(170).contains("busy"));
        assert!(describe_win32_open_error(1225).contains("refused"));
        assert!(describe_win32_open_error(121).contains("timed out"));
        assert_eq!(describe_win32_open_error(42), "Win32 error 42");
    }

    #[tokio::test]
    async fn wpods_connection_open_demux_write_and_close() {
        let (pipe, in_tx, mut out_rx) = MockMessagePipe::new();
        let mac = MacAddr6::from_str("AA:BB:CC:DD:EE:FF").unwrap();

        let open_handle = tokio::spawn(WPodsAapConnection::open_with_pipe(pipe, mac));

        let sent_open = out_rx.recv().await.unwrap();
        assert_eq!(sent_open, encode_open_request(mac));

        in_tx.send(Ok(Some(vec![0x02, 0, 0, 0, 0]))).await.unwrap();
        let conn = open_handle.await.unwrap().unwrap();

        let mut read_rx = conn.read_channel();
        let mut status_rx = conn.connection_status();
        let mut hfp_rx = conn.call_audio_active().unwrap();
        assert_eq!(*status_rx.borrow_and_update(), ConnectionStatus::Connected);
        assert!(!*hfp_rx.borrow_and_update());

        // Write packet
        conn.write(&[0x00, 0x00, 0x04, 0x00]).await.unwrap();
        assert_eq!(out_rx.recv().await.unwrap(), vec![0x00, 0x00, 0x04, 0x00]);

        // Reject oversized packet
        assert!(matches!(
            conn.write(&vec![0x01; MAX_AAP_PACKET_LEN + 1]).await,
            Err(connection::Error::WriteError { .. })
        ));

        // Inbound packet + HFP notification + ChannelClosed
        in_tx
            .send(Ok(Some(vec![0x00, 0x04, 0x00, 0x04, 0x00, 0x1D, 0x00])))
            .await
            .unwrap();
        assert_eq!(
            read_rx.recv().await.unwrap(),
            vec![0x04, 0x00, 0x04, 0x00, 0x1D, 0x00]
        );

        in_tx.send(Ok(Some(vec![0x03, 0x01]))).await.unwrap();
        hfp_rx.changed().await.unwrap();
        assert!(*hfp_rx.borrow_and_update());

        let code = 1167_i32.to_le_bytes();
        in_tx
            .send(Ok(Some(vec![0x01, code[0], code[1], code[2], code[3]])))
            .await
            .unwrap();
        status_rx.changed().await.unwrap();
        assert_eq!(
            *status_rx.borrow_and_update(),
            ConnectionStatus::Disconnected
        );
    }

    #[tokio::test]
    async fn wpods_connection_maps_nonzero_open_status_to_transport_error() {
        let (pipe, in_tx, mut out_rx) = MockMessagePipe::new();
        let mac = MacAddr6::from_str("AA:BB:CC:DD:EE:FF").unwrap();
        let open_handle = tokio::spawn(WPodsAapConnection::open_with_pipe(pipe, mac));
        let _ = out_rx.recv().await.unwrap();

        let code = 1167_i32.to_le_bytes();
        in_tx
            .send(Ok(Some(vec![0x02, code[0], code[1], code[2], code[3]])))
            .await
            .unwrap();

        let err = open_handle.await.unwrap().err().unwrap();
        assert!(matches!(
            err,
            connection::Error::TransportError { code: 1167, .. }
        ));
    }
}
