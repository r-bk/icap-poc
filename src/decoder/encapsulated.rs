use crate::{
    decoder::{skip_char, skip_whitespace},
    errors::DecoderError,
};
use std::{ops::Index, str::FromStr};
use tracing::{error, instrument, trace};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[non_exhaustive]
pub enum EncapsulatedEntity {
    ReqHdr(usize),
    ReqBody(usize),
    ResHdr(usize),
    ResBody(usize),
    NullBody(usize),
    OptBody(usize),
}

impl EncapsulatedEntity {
    fn try_from_bytes(name: &[u8], offset: &[u8]) -> Result<Self, DecoderError> {
        let offset_str = unsafe { std::str::from_utf8_unchecked(offset) };
        let off = match usize::from_str(offset_str) {
            Ok(off) => off,
            _ => {
                trace!(offset=?offset, "bad offset");
                return Err(DecoderError::BadEncapsulatedHdr("bad offset"));
            }
        };

        match name {
            b"req-hdr" => Ok(EncapsulatedEntity::ReqHdr(off)),
            b"req-body" => Ok(EncapsulatedEntity::ReqBody(off)),
            b"res-hdr" => Ok(EncapsulatedEntity::ResHdr(off)),
            b"res-body" => Ok(EncapsulatedEntity::ResBody(off)),
            b"null-body" => Ok(EncapsulatedEntity::NullBody(off)),
            b"opt-body" => Ok(EncapsulatedEntity::OptBody(off)),
            _ => {
                trace!(name=?name, "bad name");
                Err(DecoderError::BadEncapsulatedHdr("bad name"))
            }
        }
    }

    pub fn offset(&self) -> usize {
        match *self {
            Self::ReqHdr(o) => o,
            Self::ReqBody(o) => o,
            Self::ResHdr(o) => o,
            Self::ResBody(o) => o,
            Self::NullBody(o) => o,
            Self::OptBody(o) => o,
        }
    }

    #[inline]
    pub fn is_req_hdr(&self) -> bool {
        matches!(self, Self::ReqHdr(_))
    }

    #[inline]
    pub fn is_req_body(&self) -> bool {
        matches!(self, Self::ReqBody(_))
    }

    #[inline]
    pub fn is_res_hdr(&self) -> bool {
        matches!(self, Self::ResHdr(_))
    }

    #[inline]
    pub fn is_res_body(&self) -> bool {
        matches!(self, Self::ResBody(_))
    }

    #[inline]
    pub fn is_null_body(&self) -> bool {
        matches!(self, Self::NullBody(_))
    }

    #[inline]
    pub fn is_body(&self) -> bool {
        matches!(
            self,
            Self::NullBody(_) | Self::ReqBody(_) | Self::ResBody(_) | Self::OptBody(_)
        )
    }

    #[inline]
    pub fn is_hdr(&self) -> bool {
        matches!(self, Self::ReqHdr(_) | Self::ResHdr(_))
    }
}

#[derive(Debug, Clone, Default)]
#[repr(transparent)]
pub struct EeList(Vec<EncapsulatedEntity>);

impl EeList {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &EncapsulatedEntity> {
        self.0.iter()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.0.clear()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn parse_append(&mut self, buf: &[u8]) -> Result<(), DecoderError> {
        parse_encapsulated_list_(buf, &mut self.0)
    }

    #[inline]
    pub unsafe fn get_unchecked(&self, i: usize) -> &EncapsulatedEntity {
        self.0.get_unchecked(i)
    }

    #[inline]
    pub unsafe fn get_last(&self) -> &EncapsulatedEntity {
        self.0.get_unchecked(self.0.len() - 1)
    }

    #[inline]
    pub fn get_body_offset(&self) -> Result<usize, DecoderError> {
        let ee = self.0.last().unwrap(); // the list is assumed to be parsed
        let body_offset = match ee {
            EncapsulatedEntity::ReqBody(off) => *off,
            EncapsulatedEntity::ResBody(off) => *off,
            EncapsulatedEntity::NullBody(off) => *off,
            _ => {
                error!(ee_list = ?self.0, "unexpected last encapsulated entity");
                return Err(DecoderError::BadEncapsulatedHdr(
                    "unexpected last encapsulated entity",
                ));
            }
        };
        Ok(body_offset)
    }

    #[inline]
    pub fn last(&self) -> Option<&EncapsulatedEntity> {
        self.0.last()
    }
}

impl Index<usize> for EeList {
    type Output = EncapsulatedEntity;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        self.0.index(index)
    }
}

fn parse_encapsulated_list_(
    buf: &[u8],
    list: &mut Vec<EncapsulatedEntity>,
) -> Result<(), DecoderError> {
    let mut i = 0;

    let mut delimiter = false;
    while i < buf.len() {
        let slc = unsafe { buf.get_unchecked(i..) };
        let (off, ee) = parse_ee(slc, delimiter)?;
        if let Some(e) = ee {
            list.push(e);
        }
        i += off;
        delimiter = true;
    }

    if list.is_empty() {
        return Err(DecoderError::BadEncapsulatedHdr("no entities"));
    }

    for i in 0..(list.len() - 1) {
        let a = unsafe { list.get_unchecked(i) };
        let b = unsafe { list.get_unchecked(i + 1) };
        if b.offset() < a.offset() {
            return Err(DecoderError::BadEncapsulatedHdr(
                "non increasing offset sequence",
            ));
        }
    }

    Ok(())
}

#[inline]
fn skip_name(buf: &[u8], i: &mut usize) {
    while *i < buf.len() {
        let c_ref = unsafe { buf.get_unchecked(*i) };
        if !matches!(*c_ref, b'a'..=b'z' | b'-') {
            break;
        }
        *i += 1;
    }
}

#[inline]
fn skip_offset(buf: &[u8], i: &mut usize) {
    while *i < buf.len() {
        let c_ref = unsafe { buf.get_unchecked(*i) };
        if !matches!(*c_ref, b'0'..=b'9') {
            break;
        }
        *i += 1;
    }
}

#[instrument(skip(buf))]
fn parse_ee(
    buf: &[u8],
    delimiter: bool,
) -> Result<(usize, Option<EncapsulatedEntity>), DecoderError> {
    let mut i = 0;

    skip_whitespace(buf, &mut i);
    if i == buf.len() {
        return Ok((i, None));
    }

    // optional delimiter comma
    if delimiter {
        let delimiter_start = i;
        skip_char(buf, &mut i, b',');
        if i == delimiter_start {
            return Err(DecoderError::BadEncapsulatedHdr("no delimiter"));
        }
    }

    skip_whitespace(buf, &mut i);

    let name_start = i;
    skip_name(buf, &mut i);
    if i == name_start {
        trace!("empty name");
        return Err(DecoderError::BadEncapsulatedHdr("empty name"));
    }

    let name_slc = unsafe { buf.get_unchecked(name_start..i) };

    skip_whitespace(buf, &mut i);

    let equals_start = i;
    skip_char(buf, &mut i, b'=');
    if i == equals_start {
        trace!("no equals");
        return Err(DecoderError::BadEncapsulatedHdr("no equals"));
    }

    skip_whitespace(buf, &mut i);

    let offset_start = i;
    skip_offset(buf, &mut i);
    if i == offset_start {
        trace!("no offset");
        return Err(DecoderError::BadEncapsulatedHdr("no offset"));
    }

    let offset_slc = unsafe { buf.get_unchecked(offset_start..i) };

    Ok((
        i,
        Some(EncapsulatedEntity::try_from_bytes(name_slc, offset_slc)?),
    ))
}

#[cfg(test)]
mod tests {
    use super::EncapsulatedEntity::*;
    use super::*;

    fn parse_encapsulated_list(buf: &[u8]) -> Result<Vec<EncapsulatedEntity>, DecoderError> {
        let mut list = Vec::new();
        parse_encapsulated_list_(buf, &mut list)?;
        Ok(list)
    }

    #[test]
    fn test_parse_ee_list() {
        let good: Vec<(&[u8], Vec<EncapsulatedEntity>)> = vec![
            (b"req-hdr=0".as_ref(), vec![ReqHdr(0)]),
            (b"req-body=0".as_ref(), vec![ReqBody(0)]),
            (b"res-hdr=0".as_ref(), vec![ResHdr(0)]),
            (b"res-body=0".as_ref(), vec![ResBody(0)]),
            (b"null-body=0".as_ref(), vec![NullBody(0)]),
            (b"opt-body=0".as_ref(), vec![OptBody(0)]),
            (
                b"  req-hdr=0, req-body=112".as_ref(),
                vec![ReqHdr(0), ReqBody(112)],
            ),
            (
                b"res-hdr=0,  res-body=1124".as_ref(),
                vec![ResHdr(0), ResBody(1124)],
            ),
            (
                b"req-hdr=0, res-hdr = 112,  res-body=132  ",
                vec![ReqHdr(0), ResHdr(112), ResBody(132)],
            ),
            (
                b"req-hdr=0, res-hdr = 112,  null-body=537  ",
                vec![ReqHdr(0), ResHdr(112), NullBody(537)],
            ),
            (
                b"req-hdr=0,res-hdr=100,res-body=1000",
                vec![ReqHdr(0), ResHdr(100), ResBody(1000)],
            ),
        ];

        for (buf, expected) in &good {
            let el = parse_encapsulated_list(buf).unwrap();
            assert_eq!(el, *expected);
        }
    }

    #[test]
    fn test_parse_ee_list_errors() {
        let bad: Vec<(&[u8], &'static str)> = vec![
            (b"", "no entities"),
            (b"    ", "no entities"),
            (b", req-hdr=12", "empty name"),
            (b"req-hdr=0 , ", ("empty name")),
            (b"req-hdr=0,,null-body=128", "empty name"),
            (b"=0", "empty name"),
            (b"req-hdr0", "no equals"),
            (b"null-body=", "no offset"),
            (b"req-hdr=99999999999999999999999999999", "bad offset"),
            (b"reg-hdr=12", "bad name"),
            (b"res-hdr=0,res-body=", "no offset"),
            (
                b"req-hdr=0, res-hdr=1023, res-body=517",
                "non increasing offset sequence",
            ),
        ];

        for (buf, reason) in &bad {
            let res = parse_encapsulated_list(buf);
            if let Err(DecoderError::BadEncapsulatedHdr(ref r)) = res {
                assert_eq!(r, reason);
            } else {
                println!("{:?}", res);
                assert!(false);
            }
        }
    }
}
