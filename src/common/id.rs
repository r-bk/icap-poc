use cds::aformat;
use std::{
    fmt,
    sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Copy, Clone, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(transparent)]
pub struct Id(pub(crate) usize);

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(aformat!(32, "{:#X}", self.0)?.as_str())
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(aformat!(32, "Id({:#X})", self.0)?.as_str())
    }
}

// ----------------------------------------------------------------------------

#[derive(Debug)]
pub struct IdGenerator {
    gen: AtomicUsize,
}

impl IdGenerator {
    #[inline]
    pub const fn new() -> Self {
        Self {
            gen: AtomicUsize::new(1),
        }
    }

    #[cfg(test)]
    #[inline]
    pub const fn with_seed(s: usize) -> Self {
        Self {
            gen: AtomicUsize::new(s),
        }
    }

    #[inline]
    pub fn next(&self) -> Id {
        Id(self.gen.fetch_add(1, Ordering::AcqRel))
    }
}

impl Default for IdGenerator {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) static CONN_ID: IdGenerator = IdGenerator::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display() {
        let id = Id(0x777);
        assert_eq!(format!("{}", id), "0x777");
    }

    #[test]
    fn test_debug() {
        let id = Id(0xABA);
        assert_eq!(format!("{:?}", id), "Id(0xABA)");
    }

    #[test]
    fn test_id_generator() {
        let g = IdGenerator::new();
        assert_eq!(g.next().0, 1);
        assert_eq!(g.next().0, 2);

        let g = IdGenerator::with_seed(17);
        assert_eq!(g.next().0, 17);
        assert_eq!(g.next().0, 18);
    }
}
