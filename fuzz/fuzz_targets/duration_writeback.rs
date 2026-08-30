#![no_main]

use libfuzzer_sys::fuzz_target;
use pinpoint_core::source;

fuzz_target!(|data: &[u8]| {
    let Some((&requested, payload)) = data.split_first() else {
        return;
    };
    let count = usize::from(requested).min(64);
    let duration_bytes = count.saturating_mul(8);
    if payload.len() < duration_bytes {
        return;
    }
    let (raw_durations, source_bytes) = payload.split_at(duration_bytes);
    let Ok(input) = std::str::from_utf8(source_bytes) else {
        return;
    };
    let durations = raw_durations
        .chunks_exact(8)
        .map(|bytes| f64::from_le_bytes(bytes.try_into().expect("eight-byte chunk")))
        .collect::<Vec<_>>();
    let _ = source::apply_durations(input, &durations);
});
