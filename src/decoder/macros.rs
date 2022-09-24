macro_rules! next {
    ($iter:ident) => {{
        match $iter.next() {
            Some(v) => *v,
            None => return Ok(None),
        }
    }};
}

macro_rules! bail {
    ($invalid_char:expr, $state:ident, $idx:expr) => {{
        trace!(
            "invalid char {} in {:?}, idx={}",
            $invalid_char,
            $state,
            $idx
        );
        return Err(DecoderError::BadChunkHeader);
    }};
}

macro_rules! byte_map {
    ($($flag:expr,)*) => ([
        $($flag != 0,)*
    ])
}
