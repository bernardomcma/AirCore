use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use macaddr::MacAddr6;

use crate::api::connection::{self, AapBackend, AapConnection};

pub struct NoneAapBackend;

#[async_trait]
impl AapBackend for NoneAapBackend {
    async fn devices(&self) -> connection::Result<HashSet<connection::ConnectionDescriptor>> {
        unimplemented!()
    }

    async fn connect(
        &self,
        _mac_address: MacAddr6,
    ) -> connection::Result<Arc<dyn AapConnection + Send + Sync>> {
        unimplemented!()
    }
}
