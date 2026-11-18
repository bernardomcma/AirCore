use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use macaddr::MacAddr6;
use tokio::sync::{mpsc, watch};

use crate::api::connection::{
    self, AapBackend, AapConnection, ConnectionDescriptor, ConnectionStatus,
};

pub struct MockAapBackend {
    inbound: Mutex<Option<mpsc::Receiver<Vec<u8>>>>,
    outbound: Mutex<Option<mpsc::Sender<Vec<u8>>>>,
}

impl MockAapBackend {
    pub fn new(inbound: mpsc::Receiver<Vec<u8>>, outbound: mpsc::Sender<Vec<u8>>) -> Self {
        Self {
            inbound: Mutex::new(Some(inbound)),
            outbound: Mutex::new(Some(outbound)),
        }
    }
}

#[async_trait]
impl AapBackend for MockAapBackend {
    async fn devices(&self) -> connection::Result<HashSet<ConnectionDescriptor>> {
        Ok([ConnectionDescriptor {
            name: "Mock AirPods Pro 2".to_owned(),
            mac_address: MacAddr6::nil(),
        }]
        .into_iter()
        .collect())
    }

    async fn connect(
        &self,
        _mac_address: MacAddr6,
    ) -> connection::Result<Arc<dyn AapConnection + Send + Sync>> {
        Ok(Arc::new(MockAapConnection::new(
            self.inbound
                .lock()
                .unwrap()
                .take()
                .expect("connect should only be called once"),
            self.outbound
                .lock()
                .unwrap()
                .take()
                .expect("connect should only be called once"),
        )))
    }
}

pub struct MockAapConnection {
    inbound: Mutex<Option<mpsc::Receiver<Vec<u8>>>>,
    outbound: mpsc::Sender<Vec<u8>>,
    connection_status: watch::Sender<ConnectionStatus>,
    call_audio_active: watch::Sender<bool>,
}

impl MockAapConnection {
    pub fn new(inbound: mpsc::Receiver<Vec<u8>>, outbound: mpsc::Sender<Vec<u8>>) -> Self {
        Self {
            inbound: Mutex::new(Some(inbound)),
            outbound,
            connection_status: watch::channel(ConnectionStatus::Connected).0,
            call_audio_active: watch::channel(false).0,
        }
    }

    #[allow(dead_code)]
    pub fn set_connection_status(&self, status: ConnectionStatus) {
        self.connection_status.send_replace(status);
    }

    #[allow(dead_code)]
    pub fn set_call_audio_active(&self, active: bool) {
        self.call_audio_active.send_replace(active);
    }
}

#[async_trait]
impl AapConnection for MockAapConnection {
    async fn write(&self, data: &[u8]) -> connection::Result<()> {
        self.outbound.send(data.to_vec()).await.unwrap();
        Ok(())
    }

    fn read_channel(&self) -> mpsc::Receiver<Vec<u8>> {
        self.inbound
            .lock()
            .unwrap()
            .take()
            .expect("read_channel should only be called once")
    }

    fn connection_status(&self) -> watch::Receiver<ConnectionStatus> {
        self.connection_status.subscribe()
    }

    fn call_audio_active(&self) -> Option<watch::Receiver<bool>> {
        Some(self.call_audio_active.subscribe())
    }
}
