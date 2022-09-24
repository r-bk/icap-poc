use crate::{
    common::{HttpRequest, HttpResponse, IcapRequest},
    errors::DecoderError,
    header::{HeaderIndices, HeaderIndicesList},
    Method, Version,
};
use http::StatusCode;
use std::{mem::MaybeUninit, str::FromStr};
use tracing::{error, instrument, trace};

mod encapsulated;
pub use encapsulated::*;

#[macro_use]
mod macros;

mod maps;
use maps::*;

const MAX_HEADERS: usize = 128;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum DecodingStatus {
    Partial,
    Complete,
}

impl DecodingStatus {
    #[inline]
    pub fn is_partial(self) -> bool {
        self == DecodingStatus::Partial
    }
}

#[derive(Debug)]
struct RawRequestParts<'b> {
    method: &'b str,
    uri: http::Uri,
    version: u8,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct Allow {
    pub allow_204: bool,
    pub allow_206: bool,
}

impl Allow {
    pub fn add(&mut self, other: &Allow) {
        self.allow_204 = self.allow_204 || other.allow_204;
        self.allow_206 = self.allow_206 || other.allow_206;
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct ChunkHdr {
    pub chunk_len: usize,
    pub line_len: usize,
    pub ieof: bool,
}

impl ChunkHdr {
    pub unsafe fn parse(chunk_len: &[u8], line_len: usize) -> Result<Self, DecoderError> {
        let s = std::str::from_utf8_unchecked(chunk_len);
        let cl = usize::from_str_radix(s, 16).map_err(|_| {
            trace!("failed to parse chunk length: {:?}", chunk_len);
            DecoderError::BadChunkSize
        })?;
        Ok(Self {
            chunk_len: cl,
            line_len,
            ieof: false,
        })
    }
}

#[instrument(err, skip(bytes, req))]
pub(crate) fn decode_http_request(
    bytes: &[u8],
    base_ptr: usize,
    req: &mut HttpRequest,
) -> Result<Option<usize>, DecoderError> {
    match decode_raw_request_parts(bytes, base_ptr, &mut req.headers) {
        Ok(Some((parsed_len, parts))) => {
            let version = match parts.version {
                0 => http::Version::HTTP_10,
                1 => http::Version::HTTP_11,
                v => return Err(DecoderError::BadVersion(format!("bad http version: {}", v))),
            };
            let method = match http::Method::from_bytes(parts.method.as_bytes()) {
                Ok(m) => m,
                Err(e) => {
                    return Err(DecoderError::BadMethod(format!(
                        "failed to parse http method: {}",
                        e
                    )))
                }
            };
            req.method = method;
            req.uri = parts.uri;
            req.version = version;
            req.parsed_len = parsed_len;
            Ok(Some(parsed_len))
        }
        Ok(None) => Ok(None),
        Err(e) => Err(e),
    }
}

#[instrument(err, skip(bytes, req))]
pub(crate) fn decode_icap_request(
    bytes: &[u8],
    base_ptr: usize,
    req: &mut IcapRequest,
) -> Result<Option<usize>, DecoderError> {
    match decode_raw_request_parts(bytes, base_ptr, &mut req.headers) {
        Ok(Some((parsed_len, parts))) => {
            if parts.version != 10 {
                return Err(DecoderError::BadVersion(format!(
                    "bad icap version: {}",
                    parts.version
                )));
            }
            req.method = Method::from_str(parts.method)
                .map_err(|_| DecoderError::BadMethod(parts.method.into()))?;
            req.uri = parts.uri;
            req.version = Version::Icap10;
            req.parsed_len = parsed_len;
            Ok(Some(parsed_len))
        }
        Ok(None) => Ok(None),
        Err(e) => Err(e),
    }
}

#[instrument(err, skip(bytes, resp))]
pub(crate) fn decode_http_response(
    bytes: &[u8],
    base_ptr: usize,
    resp: &mut HttpResponse,
) -> Result<usize, DecoderError> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];

    let mut res = httparse::Response::new(&mut headers);

    let parsed_len = match res.parse(bytes) {
        Ok(httparse::Status::Complete(parsed_len)) => parsed_len,
        Ok(httparse::Status::Partial) => {
            error!("HTTP response partial");
            return Err(DecoderError::FailedToParseHttpRes);
        }
        Err(e) => return Err(DecoderError::BadFormat(e.to_string())),
    };

    let raw_version = res.version.unwrap();
    let version = match raw_version {
        0 => http::Version::HTTP_10,
        1 => http::Version::HTTP_11,
        _ => {
            error!(ver = raw_version, "invalid http response version");
            return Err(DecoderError::BadVersion(
                "invalid http response version".into(),
            ));
        }
    };

    let raw_status = res.code.unwrap();
    let status = match StatusCode::from_u16(raw_status) {
        Ok(s) => s,
        Err(e) => {
            error!(err = %e, "failed to parse http response status code");
            return Err(DecoderError::BadFormat(
                "bad http response status code".into(),
            ));
        }
    };

    resp.version = version;
    resp.status = status;
    resp.parsed_len = parsed_len;
    resp.headers.base_ptr = base_ptr;
    resp.headers.vec.clear();

    let n_headers = res.headers.len();
    for hdr in &headers[..n_headers] {
        let name_start = hdr.name.as_ptr() as usize - base_ptr;
        let name_end = name_start + hdr.name.len();
        let value_start = hdr.value.as_ptr() as usize - base_ptr;
        let value_end = value_start + hdr.value.len();
        resp.headers.vec.push(HeaderIndices {
            name: (name_start, name_end),
            value: (value_start, value_end),
        });
    }

    Ok(parsed_len)
}

fn decode_raw_request_parts<'b>(
    bytes: &'b [u8],
    base_ptr: usize,
    indices: &mut HeaderIndicesList,
) -> Result<Option<(usize, RawRequestParts<'b>)>, DecoderError> {
    let mut headers: [MaybeUninit<httparse::Header<'_>>; MAX_HEADERS] =
        unsafe { MaybeUninit::uninit().assume_init() };

    trace!(bytes = bytes.len(), "start");

    let mut req = httparse::Request::new(&mut []);

    let parsed_len = match req.parse_with_uninit_headers(bytes, &mut headers) {
        Ok(httparse::Status::Complete(parsed_len)) => {
            trace!("complete({})", parsed_len);
            parsed_len
        }
        Ok(httparse::Status::Partial) => {
            trace!("partial");
            return Ok(None);
        }
        Err(err) => {
            return Err(match err {
                // if invalid Token, try to determine if for method or path
                httparse::Error::Token => {
                    if req.method.is_none() {
                        DecoderError::BadMethod("invalid method token".into())
                    } else {
                        debug_assert!(req.path.is_none());
                        DecoderError::BadUri("invalid URI token".into())
                    }
                }
                other => DecoderError::BadFormat(other.to_string()),
            });
        }
    };

    let method = req.method.unwrap();
    let version = req.version.unwrap();

    let uri = match http::Uri::from_str(req.path.unwrap()) {
        Ok(v) => v,
        Err(e) => return Err(DecoderError::BadUri(e.to_string())),
    };

    let n_headers = req.headers.len();
    indices.clear();
    indices.base_ptr = base_ptr;

    // SAFETY: first n_headers are init
    for header in &headers[..n_headers] {
        let hdr = unsafe { header.assume_init_ref() };
        let name_start = hdr.name.as_ptr() as usize - base_ptr;
        let name_end = name_start + hdr.name.len();
        let value_start = hdr.value.as_ptr() as usize - base_ptr;
        let value_end = value_start + hdr.value.len();
        indices.vec.push(HeaderIndices {
            name: (name_start, name_end),
            value: (value_start, value_end),
        });
    }

    Ok(Some((
        parsed_len,
        RawRequestParts {
            method,
            uri,
            version,
        },
    )))
}

pub(crate) fn decode_allow(bytes: &[u8]) -> Result<Allow, DecoderError> {
    let mut allow_204 = false;
    let mut allow_206 = false;

    for slc in bytes.split(|b| b.is_ascii_whitespace() || *b == b',') {
        match slc {
            b"204" => allow_204 = true,
            b"206" => allow_206 = true,
            _ => continue,
        }
    }

    Ok(Allow {
        allow_204,
        allow_206,
    })
}

pub(crate) fn decode_preview(bytes: &[u8]) -> Result<usize, DecoderError> {
    for slc in bytes.split(|b| b.is_ascii_whitespace()) {
        if !slc.is_empty() && slc.is_ascii() {
            // SAFETY: only ASCII digits in slice
            let s = unsafe { std::str::from_utf8_unchecked(slc) };
            return s.parse::<usize>().map_err(|_| {
                error!(val = ?slc, "failed to parse 'Preview' value");
                DecoderError::FailedToParsePreview
            });
        }
    }
    error!("no 'Preview' size found in header");
    Err(DecoderError::FailedToParsePreview)
}

#[derive(Debug)]
enum ChunkHeaderState {
    WaitingSize,
    Size,
    WaitingDelimiter,
    WaitingExtName,
    ExtName,
    WaitingExtDelimiter,
    WaitingExtValue,
    ExtValueToken,
    ExtValueQuotedString,
}

#[derive(Debug)]
struct ExtensionIndices {
    name_start: usize,
    name_end: usize,
    val_start: usize,
    val_end: usize,
}

impl ExtensionIndices {
    fn clear(&mut self) {
        self.name_start = usize::MAX;
        self.name_end = usize::MAX;
        self.val_start = usize::MAX;
        self.val_end = usize::MAX;
    }

    #[inline]
    fn name<'s, 'b: 's>(&'s self, bytes: &'b [u8]) -> &'b [u8] {
        &bytes[self.name_start..=self.name_end]
    }

    #[inline]
    fn value<'s, 'b: 's>(&'s self, bytes: &'b [u8]) -> &'b [u8] {
        &bytes[self.val_start..=self.val_end]
    }
}

impl Default for ExtensionIndices {
    fn default() -> Self {
        Self {
            name_start: usize::MAX,
            name_end: usize::MAX,
            val_start: usize::MAX,
            val_end: usize::MAX,
        }
    }
}

#[instrument(skip(bytes))]
pub fn decode_chunk_header(bytes: &[u8]) -> Result<Option<ChunkHdr>, DecoderError> {
    use ChunkHeaderState::*;
    let mut iter = bytes.iter();
    let mut hdr = ChunkHdr::default();
    let mut state = WaitingSize;
    let mut size_start = usize::MAX;
    let mut size_end;
    let mut ext: ExtensionIndices = Default::default();
    let mut idx = 0;

    trace!("start: {:?}", bytes);

    while let Some(b) = iter.next() {
        let b = *b;
        match state {
            WaitingSize => match b {
                v if is_spht(v) => (),
                b'a'..=b'f' | b'A'..=b'F' | b'0'..=b'9' => {
                    size_start = idx;
                    state = Size;
                }
                _ => bail!(b, state, idx),
            },
            Size => match b {
                b'a'..=b'f' | b'A'..=b'F' | b'0'..=b'9' => (),
                v if is_spht(v) => {
                    size_end = idx - 1;
                    hdr = unsafe { ChunkHdr::parse(&bytes[size_start..=size_end], 0)? };
                    state = WaitingDelimiter;
                }
                b';' => {
                    size_end = idx - 1;
                    hdr = unsafe { ChunkHdr::parse(&bytes[size_start..=size_end], 0)? };
                    state = WaitingExtName;
                }
                b'\r' => match next!(iter) {
                    b'\n' => {
                        size_end = idx - 1;
                        return Ok(Some(unsafe {
                            ChunkHdr::parse(&bytes[size_start..=size_end], idx + 2)?
                        }));
                    }
                    v => bail!(v, state, idx + 1),
                },
                _ => bail!(b, state, idx),
            },
            WaitingDelimiter => match b {
                v if is_spht(v) => (),
                b';' => {
                    state = WaitingExtName;
                }
                b'\r' => match next!(iter) {
                    b'\n' => {
                        hdr.line_len = idx + 2;
                        return Ok(Some(hdr));
                    }
                    v => bail!(v, state, idx + 1),
                },
                _ => bail!(b, state, idx),
            },
            WaitingExtName => match b {
                v if is_spht(v) => (),
                v if is_token(v) => {
                    ext.name_start = idx;
                    state = ExtName;
                }
                _ => bail!(b, state, idx),
            },
            ExtName => match b {
                v if is_token(v) => (),
                v if is_spht(v) => {
                    ext.name_end = idx - 1;
                    state = WaitingExtDelimiter;
                    let ext_name = ext.name(bytes);
                    trace!(idx = idx, "parsed ext name after SP|HT: {:?}", ext_name);
                    if ext_name == b"ieof" {
                        hdr.ieof = true;
                    }
                }
                b';' => {
                    ext.name_end = idx - 1;
                    let ext_name = ext.name(bytes);
                    trace!(idx = idx, "parsed ext name after ';': {:?}", ext_name);
                    if ext_name == b"ieof" {
                        hdr.ieof = true;
                    }
                    ext.clear();
                    state = WaitingExtName;
                }
                b'=' => {
                    ext.name_end = idx - 1;
                    state = WaitingExtValue;
                    let ext_name = ext.name(bytes);
                    trace!(idx = idx, "parsed ext name after '=': {:?}", ext_name);
                    if ext_name == b"ieof" {
                        hdr.ieof = true;
                    }
                }
                b'\r' => match next!(iter) {
                    b'\n' => {
                        ext.name_end = idx - 1;
                        let ext_name = ext.name(bytes);
                        if ext_name == b"ieof" {
                            hdr.ieof = true;
                        }
                        hdr.line_len = idx + 2;
                        trace!(idx = idx, "parsed ext name after '\\r': {:?}", ext_name);
                        return Ok(Some(hdr));
                    }
                    v => bail!(v, state, idx + 1),
                },
                _ => bail!(b, state, idx),
            },
            WaitingExtDelimiter => match b {
                v if is_spht(v) => (),
                b';' => {
                    let ext_name = ext.name(bytes);
                    trace!("parsed ext name after ' ;': {:?}", ext_name);
                    if ext_name == b"ieof" {
                        hdr.ieof = true;
                    }
                    ext.clear();
                    state = WaitingExtName;
                }
                b'=' => {
                    trace!(idx = idx, "parsed ext delimiter");
                    state = WaitingExtValue;
                }
                b'\r' => match next!(iter) {
                    b'\n' => {
                        hdr.line_len = idx + 2;
                        return Ok(Some(hdr));
                    }
                    v => bail!(v, state, idx + 1),
                },
                _ => bail!(b, state, idx),
            },
            WaitingExtValue => match b {
                v if is_spht(v) => (),
                b'"' => {
                    trace!(idx = idx, "parsed ext val quoted string open delimiter");
                    ext.val_start = idx + 1;
                    state = ExtValueQuotedString;
                }
                v if is_token(v) => {
                    trace!(idx = idx, "parsed ext val token first char: {}", v);
                    ext.val_start = idx;
                    state = ExtValueToken;
                }
                _ => bail!(b, state, idx),
            },
            ExtValueQuotedString => match b {
                b'"' => {
                    if !has_quoted_prefix(bytes, idx) {
                        trace!(idx = idx, "parsed quoted string close delimiter");
                        ext.val_end = (idx - 1).max(ext.val_start);
                        trace!(idx = idx, "parsed ext val: {:?}", ext.value(bytes));
                        ext.clear();
                        state = WaitingDelimiter;
                    } else {
                        trace!("parsed quoted-pair: \\\"");
                    }
                }
                v if is_text(v) => (),
                v if v.is_ascii() && has_quoted_prefix(bytes, idx) => (),
                _ => bail!(b, state, idx),
            },
            ExtValueToken => match b {
                v if is_token(v) => (),
                v if is_spht(v) => {
                    ext.val_end = idx - 1;
                    trace!(idx = idx, "parsed ext val: {:?}", ext.value(bytes));
                    ext.clear();
                    state = WaitingDelimiter;
                }
                b';' => {
                    ext.val_end = idx - 1;
                    trace!(idx = idx, "parsed ext val: {:?}", ext.value(bytes));
                    ext.clear();
                    state = WaitingExtName;
                }
                b'\r' => match next!(iter) {
                    b'\n' => {
                        ext.val_end = idx - 1;
                        trace!(idx = idx, "parsed ext val token: {:?}", ext.value(bytes));
                        hdr.line_len = idx + 2;
                        return Ok(Some(hdr));
                    }
                    v => bail!(v, state, idx + 1),
                },
                _ => bail!(b, state, idx),
            },
        }

        idx += 1;
    }

    Ok(None)
}

#[inline]
pub fn skip_whitespace(buf: &[u8], i: &mut usize) {
    while *i < buf.len() {
        let c_ref = unsafe { buf.get_unchecked(*i) };
        if !matches!(*c_ref, b' ' | b'\t') {
            break;
        }
        *i += 1;
    }
}

#[inline]
pub fn skip_char(buf: &[u8], i: &mut usize, c: u8) {
    if *i < buf.len() {
        let c_ref = unsafe { buf.get_unchecked(*i) };
        if *c_ref == c {
            *i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    #[test]
    fn test_decode_allow() {
        let expectations: Vec<(&[u8], Allow)> = vec![
            (
                b"204",
                Allow {
                    allow_204: true,
                    allow_206: false,
                },
            ),
            (
                b"206",
                Allow {
                    allow_204: false,
                    allow_206: true,
                },
            ),
            (
                b"204, 206",
                Allow {
                    allow_204: true,
                    allow_206: true,
                },
            ),
            (
                b"trailers",
                Allow {
                    allow_204: false,
                    allow_206: false,
                },
            ),
            (
                b"204, 206, trailers",
                Allow {
                    allow_204: true,
                    allow_206: true,
                },
            ),
            (
                b"  ,, 204 , 20 ,trailers, , , , \r\n",
                Allow {
                    allow_204: true,
                    allow_206: false,
                },
            ),
            (
                b"  ,, 20 4 , 2 06, trailers,,",
                Allow {
                    allow_204: false,
                    allow_206: false,
                },
            ),
            (
                b"204206",
                Allow {
                    allow_204: false,
                    allow_206: false,
                },
            ),
        ];

        for e in &expectations {
            let allow = decode_allow(e.0).unwrap();
            assert_eq!(allow, e.1);
        }
    }

    #[test]
    #[traced_test]
    fn test_decode_chunk_header() {
        let none: Vec<&[u8]> = vec![
            b"0",
            b" 0",
            b" 0 ",
            b"  0  ",
            b"0; ieof",
            b"0 ; ieof ",
            b"  0; ieof  ",
            b"0; key=val; key; ioef",
        ];
        for e in &none {
            let res = decode_chunk_header(*e).unwrap();
            assert_eq!(res, None);
        }

        let some_no_data: Vec<(&[u8], usize, bool)> = vec![
            (b"0\r\n", 0, false),
            (b"ab\r\n", 0xAB, false),
            (b" ab\r\n", 0xAB, false),
            (b"  ab\r\n", 0xAB, false),
            (b"ab \r\n", 0xAB, false),
            (b"ab  \r\n", 0xAB, false),
            (b"bC\r\n", 0xBC, false),
            (b"bcdef\r\n", 0xBCDEF, false),
            (b"0; ieof\r\n", 0, true),
            (b"5; koko=popo; ieof; zozo\r\n", 5, true),
            (b"5; koko = popo; zozo\r\n", 5, false),
            (
                b" 1; koko=\"\\\rksaj-01q<>{}()[]\" ; ieof; zimmer\r\n",
                1,
                true,
            ),
            (b" 1; koko=\"\\\rksaj-01q<>{}()[]\" ; zimmer\r\n", 1, false),
            (b"0; key=val; key; ieof\r\n", 0, true),
            (b"0; key=\"val\\\"\"; key; ieof\r\n", 0, true),
            (b"0; key=\"\"; ieof; key=val\r\n", 0, true),
            (b"10; key=\" \tval\"; key=val\r\n", 16, false),
            (b"5; key ; key = val\r\n", 5, false),
            (b"5; key ; ieof ; key = val \r\n", 5, true),
        ];
        for e in &some_no_data {
            let res = decode_chunk_header(&e.0[..(e.0.len() - 2)]).unwrap();
            assert_eq!(res, None);

            let hdr = decode_chunk_header(e.0).unwrap().unwrap();
            assert_eq!(hdr.line_len, e.0.len());
            assert_eq!(hdr.chunk_len, e.1);
            assert_eq!(hdr.ieof, e.2);
        }

        let some: Vec<(&[u8], usize, usize, bool)> = vec![
            (b"0\r\n\r\n", 0, 3, false),
            (b" 0 \r\n\r\n", 0, 5, false),
            (b"  0  \r\n\r\n", 0, 7, false),
            (b"5\r\nabcde\r\n", 5, 3, false),
            (b"0; ieof\r\n\r\n", 0, 9, true),
            (b"2; ieof\r\nAB\r\n", 2, 9, true),
            (b"2\r\nAB\r\n", 2, 3, false),
        ];
        for e in &some {
            let hdr = decode_chunk_header(e.0).unwrap().unwrap();
            assert_eq!(hdr.chunk_len, e.1);
            assert_eq!(hdr.line_len, e.2);
            assert_eq!(hdr.ieof, e.3);
        }

        let err: Vec<&[u8]> = vec![
            b"\r",
            b"\r\n",
            b" \r\n",
            b"  \r\n",
            b";\r\n",
            b"0;\r\n",
            b"0; key=\r\n",
            b"10; key=val;\r\n",
            b"10; key=val; \r\n",
            b"10; key=v\ral",
            b"10;\rkey=val",
            b"10\r;key=val",
            b"1\r0;key=val",
            b"\r10;key=val",
        ];
        for e in &err {
            let res = decode_chunk_header(*e);
            assert!(res.is_err());
        }
    }

    // #[test]
    // fn test_decode_icap_request() {
    //     let buf = b"OPTIONS icap://my.icap.server/path?key=val ICAP/1.0\r\n\
    //         Host: my.icap.server\r\n\
    //         Encapsulated: req-hdr = 0, null-body = 100\r\n\
    //         \r\n";
    //
    //     let p_opt = decode_icap_request(buf).unwrap();
    //     assert!(p_opt.is_some());
    //     let (off, parts) = p_opt.unwrap();
    //     assert_eq!(off, buf.len());
    //     assert_eq!(parts.method, Method::Options);
    //     assert_eq!(parts.uri.path(), "/path");
    //     assert_eq!(
    //         parts.uri.scheme(),
    //         Some(Scheme::from_str("icap").unwrap()).as_ref()
    //     );
    //     assert_eq!(parts.uri.host(), Some("my.icap.server"));
    //     assert_eq!(parts.headers.len(), 2);
    //     assert_eq!(
    //         parts.headers.get("Host").cloned().unwrap(),
    //         "my.icap.server"
    //     );
    //     assert_eq!(
    //         parts.headers.get("Encapsulated").cloned().unwrap(),
    //         "req-hdr = 0, null-body = 100"
    //     );
    // }
    //
    // #[test]
    // fn test_decode_icap_request_bad_version() {
    //     let buf = b"OPTIONS http://my.http.server/path?key=val HTTP/1.0\r\n\
    //         Host: my.http.server\r\n\
    //         \r\n";
    //
    //     assert!(matches!(
    //         decode_icap_request(buf),
    //         Err(DecoderError::BadVersion(ref m)) if m == "bad icap version: 0"
    //     ));
    // }
    //
    // #[test]
    // fn test_decode_http_request_parts() {
    //     let buf = b"GET /path?key=val HTTP/1.0\r\n\
    //         Host: my.http.server\r\n\
    //         Date: Wed, 21 Oct 2015 07:28:00 GMT\r\n\
    //         \r\n";
    //
    //     let (off, parts) = decode_http_request_parts(buf).unwrap().unwrap();
    //     assert_eq!(off, buf.len());
    //     assert_eq!(parts.method, http::Method::GET);
    //     assert_eq!(parts.uri.path(), "/path");
    //     assert_eq!(parts.headers.len(), 2);
    //     assert_eq!(
    //         parts.headers.get("Host").cloned().unwrap(),
    //         "my.http.server"
    //     );
    //     assert_eq!(
    //         parts.headers.get("Date").cloned().unwrap(),
    //         "Wed, 21 Oct 2015 07:28:00 GMT"
    //     );
    // }
}
