#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct HeaderValue<'b> {
    /// The value is any ASCII
    pub(crate) value: &'b [u8],
}

impl<'b> HeaderValue<'b> {
    #[inline]
    pub(crate) fn new(value: &'b [u8]) -> Self {
        Self { value }
    }

    #[inline]
    pub fn as_bytes<'s>(&'s self) -> &'b [u8]
    where
        'b: 's,
    {
        self.value
    }
}
