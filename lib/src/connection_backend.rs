use std::{collections::HashSet, panic::Location, sync::Arc};

use async_trait::async_trait;
use macaddr::MacAddr6;

use crate::api::connection::{
    self, AapBackend, AapConnection, ConnectionDescriptor, RfcommBackend,
};

cfg_select! {
    target_os = "windows" => {
        mod windows;
    }
    _ => {
        mod none;
    }
}
#[cfg(test)]
pub(crate) mod mock;
#[allow(dead_code)]
mod wpods_aap;

/// Groups together platform specific implementations of various means of connecting to devices.
pub trait ConnectionBackends {
    type Rfcomm: RfcommBackend + Send + Sync;
    type Aap: AapBackend + Send + Sync;

    fn rfcomm(&self) -> impl Future<Output = connection::Result<Self::Rfcomm>> + Send;
    fn aap(&self) -> impl Future<Output = connection::Result<Self::Aap>> + Send;
}

#[allow(dead_code)]
pub struct UnsupportedAapBackend {
    reason: &'static str,
}

#[allow(dead_code)]
impl UnsupportedAapBackend {
    pub const fn new(reason: &'static str) -> Self {
        Self { reason }
    }
}

#[async_trait]
impl AapBackend for UnsupportedAapBackend {
    async fn devices(&self) -> connection::Result<HashSet<ConnectionDescriptor>> {
        Err(connection::Error::ServiceUnavailable {
            reason: self.reason.to_owned(),
            source: None,
            location: Location::caller(),
        })
    }

    async fn connect(
        &self,
        _mac_address: MacAddr6,
    ) -> connection::Result<Arc<dyn AapConnection + Send + Sync>> {
        Err(connection::Error::ServiceUnavailable {
            reason: self.reason.to_owned(),
            source: None,
            location: Location::caller(),
        })
    }
}

pub fn default_backends() -> Option<impl ConnectionBackends> {
    cfg_select! {
        target_os = "windows" => {
            Some(windows::PlatformConnectionBackends)
        }
        _ => {
            None::<none::NoneConnectionBackends>
        }
    }
}
