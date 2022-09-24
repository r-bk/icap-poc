use crate::errors::BadIcapMethodError;
use std::fmt::{Display, Formatter};

static METHOD_NAME: [&str; 3] = ["OPTIONS", "REQMOD", "RESPMOD"];

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash, Default)]
#[non_exhaustive]
pub enum Method {
    #[default]
    Options = 0,
    ReqMod = 1,
    RespMod = 2,
}

impl Method {
    #[inline]
    pub fn as_str(self) -> &'static str {
        unsafe { METHOD_NAME.get_unchecked(self as usize) }
    }

    #[inline]
    pub fn is_options(self) -> bool {
        self == Self::Options
    }

    #[inline]
    pub fn is_req_mod(self) -> bool {
        self == Self::ReqMod
    }

    #[inline]
    pub fn is_resp_mod(self) -> bool {
        self == Self::RespMod
    }

    #[inline]
    pub fn is_any_req(self) -> bool {
        self == Self::ReqMod || self == Self::RespMod
    }
}

impl std::str::FromStr for Method {
    type Err = BadIcapMethodError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "OPTIONS" => Ok(Self::Options),
            "REQMOD" => Ok(Self::ReqMod),
            "RESPMOD" => Ok(Self::RespMod),
            _ => Err(BadIcapMethodError),
        }
    }
}

impl Display for Method {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}
