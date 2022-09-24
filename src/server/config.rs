use crate::server::ServerCfgBuilder;

#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ServerCfg {}

impl ServerCfg {
    #[inline]
    pub fn builder() -> ServerCfgBuilder {
        ServerCfgBuilder::default()
    }
}
