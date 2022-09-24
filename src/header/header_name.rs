#[derive(Debug, Clone, Eq)]
pub struct HeaderName<'b>(pub(crate) &'b str);

impl<'b> HeaderName<'b> {
    #[inline]
    pub(crate) fn new(name: &'b str) -> Self {
        Self(name)
    }

    #[inline]
    pub fn as_str<'s>(&'s self) -> &'b str
    where
        'b: 's,
    {
        self.0
    }

    #[inline]
    pub fn as_bytes<'s>(&'s self) -> &'b [u8]
    where
        'b: 's,
    {
        self.0.as_bytes()
    }
}

impl<'a, 'b> PartialEq<HeaderName<'a>> for HeaderName<'b> {
    #[inline]
    fn eq(&self, other: &HeaderName<'a>) -> bool {
        self.0.eq_ignore_ascii_case(other.0)
    }
}

impl<'a, 'b> PartialEq<&'a str> for HeaderName<'b> {
    #[inline]
    fn eq(&self, other: &&'a str) -> bool {
        self.0.eq_ignore_ascii_case(other)
    }
}

impl<'a, 'b> PartialEq<&'a [u8]> for HeaderName<'b> {
    #[inline]
    fn eq(&self, other: &&'a [u8]) -> bool {
        self.0.as_bytes().eq_ignore_ascii_case(other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partial_eq() {
        let name = HeaderName::new("Encapsulated");

        assert_eq!(name, name.clone());
        assert_eq!(name, "encapsulateD");
        assert_eq!(name, b"EnCaPsUlAtEd".as_ref());
        assert_eq!(name.as_str(), "Encapsulated");
    }
}
