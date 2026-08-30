#![no_main]

use libfuzzer_sys::fuzz_target;
use pinpoint_core::transition::LegacyTransition;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let _ = LegacyTransition::from_json(input);
    }
});
