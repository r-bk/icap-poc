use crate::header::{Header, HeaderIndices, HeaderName, HeaderValue};
use std::{slice::Iter, str};

#[derive(Debug)]
pub struct HeaderIterator<'b> {
    pub(crate) buf: &'b [u8],
    pub(crate) iter: Iter<'b, HeaderIndices>,
}

impl<'b> Iterator for HeaderIterator<'b> {
    type Item = Header<'b>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(item) = self.iter.next() {
            let name = &self.buf[item.name.0..item.name.1];
            let value = &self.buf[item.value.0..item.value.1];
            Some(Header {
                // SAFETY: header name is printable ASCII
                name: HeaderName::new(unsafe { str::from_utf8_unchecked(name) }),
                value: HeaderValue::new(value),
            })
        } else {
            None
        }
    }
}
