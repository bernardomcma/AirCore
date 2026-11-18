mod aap;
mod rfcomm;

use crate::api::connection;

use super::ConnectionBackends;

pub struct NoneConnectionBackends;
impl ConnectionBackends for NoneConnectionBackends {
    type Rfcomm = rfcomm::NoneRfcommBackend;
    type Aap = aap::NoneAapBackend;

    async fn rfcomm(&self) -> connection::Result<Self::Rfcomm> {
        unimplemented!()
    }

    async fn aap(&self) -> connection::Result<Self::Aap> {
        unimplemented!()
    }
}
