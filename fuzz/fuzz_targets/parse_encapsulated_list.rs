#![no_main]
use icap::decoder::parse_encapsulated_list;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    parse_encapsulated_list(data).ok();
});
