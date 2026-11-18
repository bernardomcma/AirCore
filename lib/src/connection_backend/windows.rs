mod aap;
mod rfcomm;
mod utils;

use crate::{api::connection, connection_backend::ConnectionBackends};

#[derive(Default)]
pub struct PlatformConnectionBackends;

impl ConnectionBackends for PlatformConnectionBackends {
    type Rfcomm = rfcomm::WindowsRfcommBackend;
    type Aap = aap::WindowsAapBackend;

    async fn rfcomm(&self) -> connection::Result<Self::Rfcomm> {
        Ok(rfcomm::WindowsRfcommBackend)
    }

    async fn aap(&self) -> connection::Result<Self::Aap> {
        Ok(aap::WindowsAapBackend)
    }
}
