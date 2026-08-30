#![no_main]

use libfuzzer_sys::fuzz_target;
use pinpoint_core::source;

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    let analysis = source::analyze(input);
    for offset in [0, input.len() / 2, input.len()] {
        let _ = source::find_slide(&analysis, offset);
        let _ = source::complete(input, None, offset);
    }
});
