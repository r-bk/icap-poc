use crate::{
    common::{HttpRequest, IcapRequest},
    decoder::{
        self, decode_allow, decode_preview, Allow, DecodingStatus, EeList, EncapsulatedEntity::*,
    },
    errors::DecoderError,
    header::HeaderIterator,
    HttpResponse, Method,
};
use bytes::BytesMut;
use http::header::HeaderValue;
use http::StatusCode;
use std::boxed::Box;
use tracing::{error, trace, warn};

pub(crate) const RBUF_CAP: usize = 8 * 1024;
pub(crate) const HTTP_BUF_CAP: usize = RBUF_CAP;
pub type ReqCtxBox = Box<ReqCtx>;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum AdaptationDecision {
    // no adaptation required, send 204
    NoAdaptation,
    // partial adaptation of headers required, send 206
    // appends headers to the HTTP request in REQMOD, or to the HTTP response in RESPMOD
    AppendHeaders,
    // send a fully custom HTTP response
    CustomResponse,
}

#[derive(Debug)]
pub struct ReqCtx {
    pub(crate) msgs_cnt: usize,
    pub(crate) rbuf: BytesMut,
    pub(crate) http_buf: BytesMut,
    pub(crate) icap_req: IcapRequest,
    pub(crate) http_req: HttpRequest,
    pub(crate) http_res: HttpResponse,
    pub(crate) ee_list: EeList,
    pub(crate) preview: Option<usize>,
    pub(crate) null_body: bool,
    pub(crate) allow_204: bool,
    pub(crate) allow_206: bool,
    pub(crate) out_icap_status: Option<http::StatusCode>,
    pub(crate) out_icap_headers: http::HeaderMap,
    pub(crate) decision: Option<AdaptationDecision>,
    pub(crate) out_http_ver: Option<http::Version>,
    pub(crate) out_http_status: Option<http::StatusCode>,
    pub(crate) out_http_headers: http::HeaderMap,
    pub(crate) body_offset: usize,
    pub(crate) header_missing_bytes: usize,
}

impl ReqCtx {
    #[inline]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub(crate) fn new_box() -> ReqCtxBox {
        Box::new(Self::new())
    }

    pub(crate) fn init(&mut self) -> Result<DecodingStatus, DecoderError> {
        if self.decode_icap_request()?.is_partial() {
            return Ok(DecodingStatus::Partial);
        }

        self.parse_icap_headers()?;
        self.alloc_buffer_for_headers()?;
        self.check_sanity()?;

        Ok(DecodingStatus::Complete)
    }

    pub(crate) fn parse_entities(&mut self) -> Result<(), DecoderError> {
        let current_base = self.rbuf.as_ptr() as usize;

        if current_base != self.icap_req.headers.base_ptr {
            trace!(
                curr_base = current_base,
                old_base = self.icap_req.headers.base_ptr,
                "re-parsing ICAP request headers"
            );

            if self.decode_icap_request()?.is_partial() {
                error!("failed to reparse ICAP request headers");
                return Err(DecoderError::FailedToReparseIcapReq);
            }
        }

        for idx in 0..self.ee_list.len() {
            let expected_off = match idx {
                0 => 0,
                1 => self.http_req.parsed_len,
                _ => usize::MAX,
            };
            match self.ee_list[idx] {
                ReqHdr(off) => {
                    if off != expected_off {
                        warn!(
                            off = off,
                            expected = expected_off,
                            "unexpected offset in req-hdr"
                        );
                    }
                    self.decode_http_request(expected_off)?;
                }
                ResHdr(off) => {
                    if off != expected_off {
                        warn!(
                            off = off,
                            expected = expected_off,
                            "unexpected offset in res-hdr"
                        );
                    }
                    self.decode_http_response(expected_off)?;
                }
                _ => (),
            }
        }

        Ok(())
    }

    fn decode_icap_request(&mut self) -> Result<DecodingStatus, DecoderError> {
        let base_ptr = self.rbuf.as_ptr() as usize;
        match decoder::decode_icap_request(&self.rbuf, base_ptr, &mut self.icap_req) {
            Ok(Some(parsed_len)) => {
                trace!(len = parsed_len, "decoded icap request");
                Ok(DecodingStatus::Complete)
            }
            Ok(None) => {
                trace!(rbuf_len = self.rbuf.len(), "partial icap request");
                Ok(DecodingStatus::Partial)
            }
            Err(e) => {
                warn!(err = %e, "failed to decode icap request");
                Err(e)
            }
        }
    }

    fn decode_http_request(&mut self, off: usize) -> Result<(), DecoderError> {
        let base_ptr = self.rbuf.as_ptr() as usize;
        let buf = &self.rbuf[(self.icap_req.parsed_len + off)..];
        match decoder::decode_http_request(buf, base_ptr, &mut self.http_req) {
            Ok(Some(parsed_len)) => {
                trace!(len = parsed_len, "decoded http request");
                Ok(())
            }
            Ok(None) => {
                warn!(rbuf_len = self.rbuf.len(), "partial http request");
                Err(DecoderError::FailedToParseHttpReq)
            }
            Err(e) => {
                warn!(err = %e, "failed to decode http request");
                Err(e)
            }
        }
    }

    fn decode_http_response(&mut self, off: usize) -> Result<(), DecoderError> {
        let base_ptr = self.rbuf.as_ptr() as usize;
        let buf = &self.rbuf[(self.icap_req.parsed_len + off)..];
        let parsed_len = decoder::decode_http_response(buf, base_ptr, &mut self.http_res)?;
        trace!(len = parsed_len, "decoded http response");
        Ok(())
    }

    fn parse_icap_headers(&mut self) -> Result<(), DecoderError> {
        let mut ee_list = std::mem::take(&mut self.ee_list);
        ee_list.clear();

        let mut preview = None;
        let mut allow: Option<Allow> = None;
        let mut null_body = false;

        for h in self.icap_req_headers() {
            if h.name == "Encapsulated" {
                ee_list.parse_append(h.value.as_bytes())?;
            } else if h.name == "Preview" {
                preview = Some(decode_preview(h.value.as_bytes())?);
                trace!(preview = preview, "parsed Preview");
            } else if h.name == "Allow" {
                let tmp = decode_allow(h.value.as_bytes())?;
                trace!(allow = ?tmp, "parsed Allow");
                match allow {
                    Some(ref mut a) => a.add(&tmp),
                    None => allow = Some(tmp),
                }
            }
        }

        if !ee_list.is_empty() {
            trace!(ee_list = ?ee_list, "parsed EeList");

            // SAFETY: EeList::parse ensures the list is non-empty
            let last_ee = unsafe { ee_list.get_last() };
            if !last_ee.is_body() {
                error!(ee_list = ?ee_list, "last EeList entity is not a body");
                self.ee_list = ee_list;
                return Err(DecoderError::BadEncapsulatedHdr(
                    "last entity is not a body",
                ));
            }

            null_body = last_ee.is_null_body();
            trace!(null_body = null_body, "parsed 'null_body'");
        } else if self.icap_req.method != Method::Options {
            self.ee_list = ee_list;
            error!("no 'Encapsulated' header found");
            return Err(DecoderError::NoEncapsulatedHdr);
        } else {
            trace!("no 'Encapsulated' header found");
        }

        if preview.is_none() {
            trace!("no 'Preview' header found");
        }

        match allow {
            Some(a) => {
                self.allow_204 = a.allow_204;
                self.allow_206 = a.allow_206;
            }
            None => trace!("no 'Allow' header found"),
        }

        self.ee_list = ee_list;
        self.preview = preview;
        self.null_body = null_body;

        Ok(())
    }

    fn alloc_buffer_for_headers(&mut self) -> Result<(), DecoderError> {
        let (body_offset, missing_bytes) =
            if let Some(body_offset) = self.ee_list.get_body_offset()? {
                trace!(offset = body_offset, "found body offset");

                let already_read_bytes = self.rbuf.len();

                debug_assert!(self.icap_req.parsed_len <= already_read_bytes);
                let header_read_bytes = already_read_bytes - self.icap_req.parsed_len;

                let missing_bytes = if body_offset > header_read_bytes {
                    body_offset - header_read_bytes
                } else {
                    0
                };

                trace!(
                    buf_len = already_read_bytes,
                    header_already_read = header_read_bytes,
                    missing_bytes = missing_bytes,
                    "calculated missing header bytes"
                );

                if missing_bytes > 0 {
                    self.rbuf.reserve(missing_bytes);
                }

                (body_offset, missing_bytes)
            } else {
                (usize::MAX, 0)
            };

        self.body_offset = body_offset;
        self.header_missing_bytes = missing_bytes;

        Ok(())
    }

    fn check_sanity(&self) -> Result<(), DecoderError> {
        let mut is_req = false;

        let good_ee = match self.icap_req.method {
            Method::Options => {
                if self.ee_list.is_empty() {
                    true
                } else if self.ee_list.len() != 1 {
                    false
                } else {
                    matches!(self.ee_list[0], NullBody(off) if off == 0)
                }
            }
            Method::ReqMod => {
                is_req = true;
                if self.ee_list.len() != 2 {
                    false
                } else {
                    matches!(self.ee_list[0], ReqHdr(off) if off == 0)
                        && matches!(self.ee_list[1], ReqBody(_) | NullBody(_))
                }
            }
            Method::RespMod => {
                is_req = true;
                match self.ee_list.len() {
                    2 => {
                        matches!(self.ee_list[0], ResHdr(off) if off == 0)
                            && matches!(self.ee_list[1], ResBody(_) | NullBody(_))
                    }
                    3 => {
                        matches!(self.ee_list[0], ReqHdr(off) if off == 0)
                            && matches!(self.ee_list[1], ResHdr(_))
                            && matches!(self.ee_list[2], ResBody(_) | NullBody(_))
                    }
                    _ => false,
                }
            }
        };

        if !good_ee {
            error!(ee_list = ?self.ee_list, "unexpected ee_list in {} request", self.icap_req.method);
            return Err(DecoderError::BadEncapsulatedHdr("unexpected ee_list"));
        }

        if is_req && !self.null_body {
            if !self.allow_206 {
                error!("206 is not allowed on request with body");
                return Err(DecoderError::NoAllow206);
            }
            let is_zero_preview = match self.preview {
                Some(prv) => prv == 0,
                None => false,
            };
            if !is_zero_preview {
                error!("'Preview: 0' is not found on request with body");
                return Err(DecoderError::NoPreview0);
            }
        }

        Ok(())
    }

    #[inline]
    pub fn icap_req(&self) -> &IcapRequest {
        &self.icap_req
    }

    #[inline]
    pub fn icap_req_headers(&self) -> HeaderIterator<'_> {
        HeaderIterator {
            buf: &self.rbuf,
            iter: self.icap_req.headers.vec.iter(),
        }
    }

    #[inline]
    pub fn http_req(&self) -> Option<&HttpRequest> {
        if self.http_req.parsed_len != 0 {
            Some(&self.http_req)
        } else {
            None
        }
    }

    #[inline]
    pub fn http_req_headers(&self) -> HeaderIterator<'_> {
        HeaderIterator {
            buf: &self.rbuf,
            iter: self.http_req.headers.vec.iter(),
        }
    }

    #[inline]
    pub fn http_res(&self) -> Option<&HttpResponse> {
        if self.http_res.parsed_len != 0 {
            Some(&self.http_res)
        } else {
            None
        }
    }

    #[inline]
    pub fn http_res_headers(&self) -> HeaderIterator<'_> {
        HeaderIterator {
            buf: &self.rbuf,
            iter: self.http_res.headers.vec.iter(),
        }
    }

    #[inline]
    pub fn allow_204(&self) -> bool {
        self.allow_204
    }

    #[inline]
    pub fn allow_206(&self) -> bool {
        self.allow_206
    }

    #[inline]
    pub fn set_icap_status(&mut self, status: StatusCode) {
        self.out_icap_status = Some(status);
    }

    #[inline]
    pub fn set_http_status(&mut self, status: StatusCode) {
        self.out_http_status = Some(status);
    }

    #[inline]
    pub fn set_decision(&mut self, decision: AdaptationDecision) {
        self.decision = Some(decision);
    }

    #[inline]
    pub fn append_icap_res_header(&mut self, name: &'static str, val: &'static str) {
        self.out_icap_headers
            .append(name, HeaderValue::from_static(val));
    }

    #[inline]
    pub fn append_icap_res_header_val(&mut self, name: &'static str, val: HeaderValue) {
        self.out_icap_headers.append(name, val);
    }

    #[inline]
    pub fn append_http_header(&mut self, name: &'static str, val: &'static str) {
        self.out_http_headers
            .append(name, HeaderValue::from_static(val));
    }

    #[inline]
    pub fn append_http_header_val(&mut self, name: &'static str, val: HeaderValue) {
        self.out_http_headers.append(name, val);
    }

    pub(crate) fn clear(&mut self) {
        self.rbuf.clear();
        self.icap_req.clear();
        self.http_req.clear();
        self.http_res.clear();
        self.ee_list.clear();
        self.preview = None;
        self.null_body = false;
        self.allow_204 = false;
        self.allow_206 = false;
        self.decision = None;
        self.out_icap_headers.clear();
        self.out_icap_status = None;
        self.out_http_status = None;
        self.out_http_headers.clear();
        self.out_http_ver = None;
        self.body_offset = 0;
        self.header_missing_bytes = 0;
    }

    pub(crate) fn ensure_options_headers(&mut self) {
        for (k, v) in &[
            ("Encapsulated", "null-body=0"),
            ("Methods", "REQMOD, RESPMOD"),
            ("Allow", "204, 206"),
            ("ISTag", "\"r-bk-icap\""),
            ("Server", "r-bk/icap"),
            ("Preview", "0"),
            ("Transfer-Preview", "*"),
            ("Connection", "keep-alive"),
        ] {
            self.out_icap_headers
                .entry(*k)
                .or_insert(HeaderValue::from_static(v));
        }
    }

    pub(crate) fn ensure_204_headers(&mut self) {
        for (k, v) in &[
            ("Encapsulated", "null-body=0"),
            ("ISTag", "\"r-bk-icap\""),
            ("Server", "r-bk/icap"),
            ("Connection", "keep-alive"),
        ] {
            self.out_icap_headers
                .entry(*k)
                .or_insert(HeaderValue::from_static(v));
        }
    }

    pub(crate) fn ensure_response_headers(&mut self) {
        for (k, v) in &[
            ("ISTag", "\"r-bk-icap\""),
            ("Server", "r-bk/icap"),
            ("Connection", "keep-alive"),
        ] {
            self.out_icap_headers
                .entry(*k)
                .or_insert(HeaderValue::from_static(v));
        }
    }
}

impl Default for ReqCtx {
    #[inline]
    fn default() -> Self {
        Self {
            msgs_cnt: 0,
            rbuf: BytesMut::with_capacity(RBUF_CAP),
            http_buf: BytesMut::with_capacity(HTTP_BUF_CAP),
            icap_req: IcapRequest::default(),
            http_req: HttpRequest::default(),
            http_res: HttpResponse::default(),
            ee_list: EeList::default(),
            preview: None,
            null_body: false,
            allow_204: false,
            allow_206: false,
            decision: None,
            out_icap_headers: Default::default(),
            out_icap_status: Default::default(),
            out_http_status: None,
            out_http_headers: Default::default(),
            out_http_ver: None,
            body_offset: 0,
            header_missing_bytes: 0,
        }
    }
}
