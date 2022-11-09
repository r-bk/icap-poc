use crate::header::HeaderIndicesList;

#[derive(Debug, Default)]
pub struct HttpRequest {
    pub method: http::Method,
    pub uri: http::Uri,
    pub version: http::Version,
    pub(crate) headers: HeaderIndicesList,
    pub(crate) parsed_len: usize,
}

impl HttpRequest {
    #[inline]
    pub fn is_parsed(&self) -> bool {
        self.parsed_len != 0
    }

    pub(crate) fn clear(&mut self) {
        self.method = Default::default();
        self.headers.clear();
        self.parsed_len = 0;
    }
}
