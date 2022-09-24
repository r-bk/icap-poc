use crate::errors::BadIcapVersionError;
use std::fmt::{Display, Formatter};

static VERSION_NAME: [&str; 1] = ["ICAP/1.0"];

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash, Default)]
#[non_exhaustive]
pub enum Version {
    #[default]
    Icap10 = 0,
}

impl Version {
    #[inline]
    pub fn as_str(self) -> &'static str {
        unsafe { VERSION_NAME.get_unchecked(self as usize) }
    }
}

impl std::str::FromStr for Version {
    type Err = BadIcapVersionError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ICAP/1.0" => Ok(Self::Icap10),
            _ => Err(BadIcapVersionError),
        }
    }
}

impl Display for Version {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}
