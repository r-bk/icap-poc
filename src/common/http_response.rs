use crate::header::HeaderIndicesList;

#[derive(Debug, Default)]
pub struct HttpResponse {
    pub version: http::Version,
    pub status: http::StatusCode,
    pub(crate) headers: HeaderIndicesList,
    pub(crate) parsed_len: usize,
}

impl HttpResponse {
    #[inline]
    pub fn is_parsed(&self) -> bool {
        self.parsed_len != 0
    }

    pub(crate) fn clear(&mut self) {
        self.version = Default::default();
        self.status = Default::default();
        self.headers.clear();
        self.parsed_len = 0;
    }
}
