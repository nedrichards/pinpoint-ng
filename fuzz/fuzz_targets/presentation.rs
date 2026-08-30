#![no_main]

use libfuzzer_sys::fuzz_target;
use pinpoint_core::{presentation, source};

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };

    let _ = presentation::parse(input, None, false);
    let _ = presentation::parse(input, None, true);
    let analysis = source::analyze(input);
    let _ = source::find_slide(&analysis, input.len());
});
