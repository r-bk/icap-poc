use crate::server::ServerCfg;
use std::sync::Arc;

#[derive(Debug, Default)]
#[non_exhaustive]
pub struct ServerCfgBuilder {}

impl ServerCfgBuilder {
    pub fn build(self) -> Arc<ServerCfg> {
        Arc::new(ServerCfg {})
    }
}
