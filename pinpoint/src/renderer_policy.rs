use std::path::Path;

fn drm_device_matches(vendor_id: &str, device_id: &str) -> bool {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(number) = name.strip_prefix("card") else {
            return false;
        };
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        let read_id = |name: &str| {
            std::fs::read_to_string(entry.path().join(Path::new("device")).join(name))
                .ok()
                .map(|value| value.trim().to_ascii_lowercase())
        };
        read_id("vendor").as_deref() == Some(vendor_id)
            && read_id("device").as_deref() == Some(device_id)
    })
}

pub fn apply() {
    if std::env::var_os("GSK_RENDERER").is_some() {
        return;
    }
    if drm_device_matches("0x8086", "0x9a49") {
        // SAFETY: This runs at the first line of main, before GStreamer, GLib,
        // GTK or any application thread is initialized. No concurrent code can
        // read or mutate the process environment yet.
        unsafe { std::env::set_var("GSK_RENDERER", "gl") };
    }
}
