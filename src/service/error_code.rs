use thiserror::Error;

#[derive(Error, Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Default)]
#[error("error code: {0:#X}")]
pub struct ErrorCode(pub u16);

impl From<u16> for ErrorCode {
    #[inline]
    fn from(ec: u16) -> Self {
        Self(ec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_display() {
        let ec = ErrorCode(0x777);
        assert_eq!(ec.to_string(), "error code: 0x777");
    }
}
