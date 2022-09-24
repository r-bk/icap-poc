use crate::{header::HeaderIndicesList, Method, Version};

#[derive(Debug, Default)]
pub struct IcapRequest {
    pub method: Method,
    pub uri: http::Uri,
    pub version: Version,
    pub(crate) headers: HeaderIndicesList,
    pub(crate) parsed_len: usize,
}

impl IcapRequest {
    pub(crate) fn clear(&mut self) {
        self.method = Method::Options;
        self.headers.clear();
        self.parsed_len = 0;
    }
}
