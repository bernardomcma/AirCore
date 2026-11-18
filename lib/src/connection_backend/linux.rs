mod rfcomm;

use crate::{
    api::connection,
    connection_backend::{ConnectionBackends, UnsupportedAapBackend},
};

#[derive(Default)]
pub struct PlatformConnectionBackends;

impl ConnectionBackends for PlatformConnectionBackends {
    type Rfcomm = rfcomm::BluerRfcommBackend;
    type Aap = UnsupportedAapBackend;

    async fn rfcomm(&self) -> connection::Result<Self::Rfcomm> {
        rfcomm::BluerRfcommBackend::new().await
    }

    async fn aap(&self) -> connection::Result<Self::Aap> {
        Ok(UnsupportedAapBackend::new(
            "AirPods Pro 2 AAP backend is only supported on Windows via WPods.Service (use Demo Mode on Linux)",
        ))
    }
}
