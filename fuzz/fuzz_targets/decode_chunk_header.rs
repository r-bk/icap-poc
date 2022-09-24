#![no_main]
use icap::decoder::decode_chunk_header;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    decode_chunk_header(data).ok();
});
