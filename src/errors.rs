use std::{fmt, io};
use thiserror::Error;

#[derive(Error, Debug, Copy, Clone)]
#[error("bad ICAP method")]
#[non_exhaustive]
pub struct BadIcapMethodError;

#[derive(Error, Debug, Copy, Clone)]
#[error("bad ICAP version")]
#[non_exhaustive]
pub struct BadIcapVersionError;

#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecoderError {
    #[error("bad format: {0}")]
    BadFormat(String),
    #[error("bad method: {0}")]
    BadMethod(String),
    #[error("bad uri: {0}")]
    BadUri(String),
    #[error("bad version")]
    BadVersion(String),
    #[error("bad encapsulated header: {0}")]
    BadEncapsulatedHdr(&'static str),
    #[error("'Encapsulated' header not found")]
    NoEncapsulatedHdr,
    #[error("failed to re-parse icap_req")]
    FailedToReparseIcapReq,
    #[error("failed to parse http_req")]
    FailedToParseHttpReq,
    #[error("failed to parse http_res")]
    FailedToParseHttpRes,
    #[error("failed to parse 'Preview' header")]
    FailedToParsePreview,
    #[error("206 response not allowed")]
    NoAllow206,
    #[error("no 'Preview: 0' found")]
    NoPreview0,
    #[error("bad chunk header")]
    BadChunkHeader,
    #[error("failed to parse chunk size")]
    BadChunkSize,
}

#[derive(Debug, Error)]
pub(crate) enum ConnectionError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("fmt error: {0}")]
    Fmt(#[from] fmt::Error),
    #[error(transparent)]
    Decoder(#[from] DecoderError),
}
