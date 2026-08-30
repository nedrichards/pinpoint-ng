use gtk::gdk::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

const MIN_TEXTURE_BUDGET: u64 = 64 * 1024 * 1024;
const MAX_TEXTURE_BUDGET: u64 = 512 * 1024 * 1024;
const ASSUMED_TEXTURE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TRACKED_ASSETS: usize = 1_024;
const MAX_FAILURES: usize = 1_024;
const MAX_SVG_CACHE: usize = 64;
const MAX_SVG_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SVG_ELEMENTS: usize = 100_000;
const MAX_SVG_DEPTH: usize = 256;
const IMAGE_LOAD_TIMEOUT: Duration = Duration::from_secs(20);
const SVG_LOAD_TIMEOUT: Duration = Duration::from_secs(10);

type ReadyCallback = Box<dyn FnOnce(Result<(), String>)>;

#[derive(Clone)]
pub struct AssetStore(Rc<Inner>);

struct CachedTexture {
    texture: gdk::Texture,
    bytes: u64,
    last_used: u64,
}

struct Inner {
    textures: RefCell<HashMap<PathBuf, CachedTexture>>,
    queue: RefCell<VecDeque<PathBuf>>,
    callbacks: RefCell<HashMap<PathBuf, Vec<ReadyCallback>>>,
    in_flight: RefCell<HashMap<PathBuf, gio::Cancellable>>,
    path_generations: RefCell<HashMap<PathBuf, u64>>,
    failures: RefCell<HashMap<PathBuf, String>>,
    svg: RefCell<HashMap<PathBuf, Rc<rsvg::SvgHandle>>>,
    use_serial: Cell<u64>,
    generation: Cell<u64>,
    active_loaders: Cell<usize>,
    loader_limit: usize,
    texture_bytes: Cell<u64>,
    texture_budget: u64,
    load_delay_ms: Cell<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub textures: usize,
    pub pending: usize,
    pub failures: usize,
    pub svg_sources: usize,
    pub active_loaders: usize,
    pub loader_limit: usize,
    pub texture_bytes: u64,
    pub texture_budget: u64,
}

fn read_number(path: &str) -> Option<u64> {
    std::fs::read_to_string(path)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
}

fn effective_memory_bytes() -> u64 {
    let system = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix("MemTotal:")
                    .and_then(|value| value.split_whitespace().next())
                    .and_then(|value| value.parse::<u64>().ok())
                    .map(|kibibytes| kibibytes.saturating_mul(1024))
            })
        })
        .unwrap_or(4 * 1024 * 1024 * 1024);
    [
        read_number("/sys/fs/cgroup/memory.max"),
        read_number("/sys/fs/cgroup/memory/memory.limit_in_bytes"),
    ]
    .into_iter()
    .flatten()
    .fold(system, u64::min)
}

fn adaptive_texture_budget() -> u64 {
    (effective_memory_bytes() / 48).clamp(MIN_TEXTURE_BUDGET, MAX_TEXTURE_BUDGET)
}

fn adaptive_loader_limit() -> usize {
    match std::thread::available_parallelism().map_or(1, usize::from) {
        0..=2 => 1,
        3..=8 => 2,
        _ => 3,
    }
}

impl Default for AssetStore {
    fn default() -> Self {
        Self(Rc::new(Inner {
            textures: RefCell::new(HashMap::new()),
            queue: RefCell::new(VecDeque::new()),
            callbacks: RefCell::new(HashMap::new()),
            in_flight: RefCell::new(HashMap::new()),
            path_generations: RefCell::new(HashMap::new()),
            failures: RefCell::new(HashMap::new()),
            svg: RefCell::new(HashMap::new()),
            use_serial: Cell::new(0),
            generation: Cell::new(0),
            active_loaders: Cell::new(0),
            loader_limit: adaptive_loader_limit(),
            texture_bytes: Cell::new(0),
            texture_budget: adaptive_texture_budget(),
            load_delay_ms: Cell::new(0),
        }))
    }
}

impl AssetStore {
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub fn set_load_delay(&self, delay: Duration) {
        self.0
            .load_delay_ms
            .set(delay.as_millis().min(u128::from(u64::MAX)) as u64);
    }

    pub fn clear(&self) {
        self.0
            .generation
            .set(self.0.generation.get().wrapping_add(1));
        for cancellable in self.0.in_flight.borrow().values() {
            cancellable.cancel();
        }
        self.0.textures.borrow_mut().clear();
        self.0.queue.borrow_mut().clear();
        self.0.callbacks.borrow_mut().clear();
        self.0.in_flight.borrow_mut().clear();
        self.0.path_generations.borrow_mut().clear();
        self.0.failures.borrow_mut().clear();
        self.0.svg.borrow_mut().clear();
        self.0.active_loaders.set(0);
        self.0.texture_bytes.set(0);
    }

    pub fn invalidate(&self, path: &Path) {
        let next_generation = self
            .0
            .path_generations
            .borrow()
            .get(path)
            .copied()
            .unwrap_or(0)
            .wrapping_add(1);
        self.0
            .path_generations
            .borrow_mut()
            .insert(path.to_path_buf(), next_generation);
        if let Some(cancellable) = self.0.in_flight.borrow().get(path) {
            cancellable.cancel();
        }
        self.0.queue.borrow_mut().retain(|queued| queued != path);
        self.0.callbacks.borrow_mut().remove(path);
        self.0.failures.borrow_mut().remove(path);
        self.0.svg.borrow_mut().remove(path);
        if let Some(cached) = self.0.textures.borrow_mut().remove(path) {
            self.0
                .texture_bytes
                .set(self.0.texture_bytes.get().saturating_sub(cached.bytes));
        }
    }

    pub fn texture(&self, path: &Path) -> Option<gdk::Texture> {
        let serial = self.0.use_serial.get().wrapping_add(1);
        self.0.use_serial.set(serial);
        self.0.textures.borrow_mut().get_mut(path).map(|cached| {
            cached.last_used = serial;
            cached.texture.clone()
        })
    }

    pub fn prefetch_slots(&self) -> usize {
        (self.0.texture_budget / ASSUMED_TEXTURE_BYTES).clamp(3, 64) as usize
    }

    pub fn media_prefetch_slots(&self) -> usize {
        (self.prefetch_slots() / 4).clamp(2, 8)
    }

    pub fn prefetch_texture<F>(&self, path: PathBuf, ready: F)
    where
        F: FnOnce(Result<(), String>) + 'static,
    {
        if self.0.textures.borrow().contains_key(&path) {
            ready(Ok(()));
            return;
        }
        if let Some(error) = self.0.failures.borrow().get(&path) {
            ready(Err(error.clone()));
            return;
        }
        if self.0.callbacks.borrow().len() >= MAX_TRACKED_ASSETS {
            ready(Err("too many distinct image loads are pending".into()));
            return;
        }

        let mut callbacks = self.0.callbacks.borrow_mut();
        if let Some(waiters) = callbacks.get_mut(&path) {
            waiters.push(Box::new(ready));
            return;
        }
        callbacks.insert(path.clone(), vec![Box::new(ready)]);
        drop(callbacks);
        self.0.queue.borrow_mut().push_back(path);
        self.pump();
    }

    fn pump(&self) {
        let queued = self.0.queue.borrow().len();
        for _ in 0..queued {
            if self.0.active_loaders.get() >= self.0.loader_limit {
                break;
            }
            let Some(path) = self.0.queue.borrow_mut().pop_front() else {
                break;
            };
            if self.0.in_flight.borrow().contains_key(&path) {
                self.0.queue.borrow_mut().push_back(path);
                continue;
            }
            self.0
                .active_loaders
                .set(self.0.active_loaders.get().saturating_add(1));
            let generation = self.0.generation.get();
            let path_generation = self
                .0
                .path_generations
                .borrow()
                .get(&path)
                .copied()
                .unwrap_or(0);
            let cancellable = gio::Cancellable::new();
            let load_delay = Duration::from_millis(self.0.load_delay_ms.get());
            self.0
                .in_flight
                .borrow_mut()
                .insert(path.clone(), cancellable.clone());
            let store = self.clone();
            glib::MainContext::default().spawn_local(async move {
                let result = async {
                    if !load_delay.is_zero() {
                        glib::timeout_future(load_delay).await;
                    }
                    if cancellable.is_cancelled() {
                        return Err("image load cancelled".to_owned());
                    }
                    let mut loader = glycin::Loader::new(gio::File::for_path(&path));
                    loader.cancellable(cancellable.clone());
                    let timeout_cancellable = cancellable.clone();
                    let timeout = glib::timeout_add_local_once(IMAGE_LOAD_TIMEOUT, move || {
                        timeout_cancellable.cancel();
                    });
                    let loaded = loader.load().await;
                    if !cancellable.is_cancelled() {
                        timeout.remove();
                    }
                    let image = loaded.map_err(|error| {
                        if cancellable.is_cancelled() {
                            "image load cancelled or timed out".to_owned()
                        } else {
                            error.to_string()
                        }
                    })?;
                    let frame = image
                        .next_frame()
                        .await
                        .map_err(|error| error.to_string())?;
                    Ok::<gdk::Texture, String>(frame.texture())
                }
                .await;

                if store.0.generation.get() != generation {
                    return;
                }
                store.0.in_flight.borrow_mut().remove(&path);
                store
                    .0
                    .active_loaders
                    .set(store.0.active_loaders.get().saturating_sub(1));
                if store
                    .0
                    .path_generations
                    .borrow()
                    .get(&path)
                    .copied()
                    .unwrap_or(0)
                    != path_generation
                {
                    store.pump();
                    return;
                }
                match &result {
                    Ok(texture) => {
                        let bytes = (texture.width().max(0) as u64)
                            .saturating_mul(texture.height().max(0) as u64)
                            .saturating_mul(4);
                        if bytes > store.0.texture_budget {
                            let error = format!(
                                "decoded image requires {} MiB, exceeding the {} MiB texture budget",
                                bytes / 1024 / 1024,
                                store.0.texture_budget / 1024 / 1024
                            );
                            if store.0.failures.borrow().len() < MAX_FAILURES {
                                store.0.failures.borrow_mut().insert(path.clone(), error.clone());
                            }
                            if let Some(callbacks) =
                                store.0.callbacks.borrow_mut().remove(&path)
                            {
                                for callback in callbacks {
                                    callback(Err(error.clone()));
                                }
                            }
                            store.pump();
                            return;
                        }
                        let serial = store.0.use_serial.get().wrapping_add(1);
                        store.0.use_serial.set(serial);
                        store
                            .0
                            .texture_bytes
                            .set(store.0.texture_bytes.get().saturating_add(bytes));
                        store.0.textures.borrow_mut().insert(
                            path.clone(),
                            CachedTexture {
                                texture: texture.clone(),
                                bytes,
                                last_used: serial,
                            },
                        );
                        store.trim(&path);
                    }
                    Err(error) => {
                        if store.0.failures.borrow().len() < MAX_FAILURES {
                            store
                                .0
                                .failures
                                .borrow_mut()
                                .insert(path.clone(), error.clone());
                        }
                    }
                }
                let callbacks = store.0.callbacks.borrow_mut().remove(&path);
                if let Some(callbacks) = callbacks {
                    for callback in callbacks {
                        callback(result.clone().map(|_| ()));
                    }
                }
                store.pump();
            });
        }
    }

    fn trim(&self, keep: &Path) {
        loop {
            if self.0.texture_bytes.get() <= self.0.texture_budget {
                break;
            }
            let oldest = self
                .0
                .textures
                .borrow()
                .iter()
                .filter(|(path, _)| path.as_path() != keep)
                .min_by_key(|(_, cached)| cached.last_used)
                .map(|(path, _)| path.clone());
            let Some(oldest) = oldest else {
                break;
            };
            if let Some(cached) = self.0.textures.borrow_mut().remove(&oldest) {
                self.0
                    .texture_bytes
                    .set(self.0.texture_bytes.get().saturating_sub(cached.bytes));
            }
        }
    }

    pub fn svg(&self, path: &Path) -> Result<Rc<rsvg::SvgHandle>, String> {
        if let Some(handle) = self.0.svg.borrow().get(path) {
            return Ok(handle.clone());
        }
        let size = std::fs::metadata(path)
            .map_err(|error| error.to_string())?
            .len();
        if size > MAX_SVG_BYTES {
            return Err("SVG exceeds the 16 MiB safety limit".into());
        }
        let source = std::fs::read(path).map_err(|error| error.to_string())?;
        validate_svg_source(&source)?;
        let file = gio::File::for_path(path);
        let cancellable = gio::Cancellable::new();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let timeout_cancellable = cancellable.clone();
        let timeout = std::thread::spawn(move || {
            if finished_rx.recv_timeout(SVG_LOAD_TIMEOUT).is_err() {
                timeout_cancellable.cancel();
            }
        });
        let result = rsvg::Loader::new().read_file(&file, Some(&cancellable));
        let _ = finished_tx.send(());
        let _ = timeout.join();
        let handle = result.map_err(|error| {
            if cancellable.is_cancelled() {
                "SVG load timed out".to_owned()
            } else {
                error.to_string()
            }
        });
        let handle = match handle {
            Ok(handle) => Rc::new(handle),
            Err(error) => {
                if self.0.failures.borrow().len() < MAX_FAILURES {
                    self.0
                        .failures
                        .borrow_mut()
                        .insert(path.to_path_buf(), error.clone());
                }
                return Err(error);
            }
        };
        let mut cache = self.0.svg.borrow_mut();
        if cache.len() >= MAX_SVG_CACHE {
            cache.clear();
        }
        cache.insert(path.to_path_buf(), handle.clone());
        Ok(handle)
    }

    pub fn stats(&self) -> Stats {
        Stats {
            textures: self.0.textures.borrow().len(),
            pending: self.0.callbacks.borrow().len(),
            failures: self.0.failures.borrow().len(),
            svg_sources: self.0.svg.borrow().len(),
            active_loaders: self.0.active_loaders.get(),
            loader_limit: self.0.loader_limit,
            texture_bytes: self.0.texture_bytes.get(),
            texture_budget: self.0.texture_budget,
        }
    }

    pub fn failure(&self, path: &Path) -> Option<String> {
        self.0.failures.borrow().get(path).cloned()
    }

    pub fn failed_paths(&self) -> Vec<PathBuf> {
        self.0.failures.borrow().keys().cloned().collect()
    }
}

fn validate_svg_source(source: &[u8]) -> Result<(), String> {
    if source
        .windows(9)
        .any(|bytes| bytes.eq_ignore_ascii_case(b"<!doctype"))
    {
        return Err("SVG document types are not supported".into());
    }
    let mut depth = 0usize;
    let mut elements = 0usize;
    let mut cursor = 0usize;
    while let Some(relative) = source[cursor..].iter().position(|byte| *byte == b'<') {
        let start = cursor + relative;
        let Some(relative_end) = source[start..].iter().position(|byte| *byte == b'>') else {
            return Err("SVG contains an unterminated tag".into());
        };
        let end = start + relative_end;
        let tag = &source[start + 1..end];
        cursor = end + 1;
        if tag.starts_with(b"!") || tag.starts_with(b"?") {
            continue;
        }
        let closing = tag.starts_with(b"/");
        let self_closing = tag.iter().rev().find(|byte| !byte.is_ascii_whitespace()) == Some(&b'/');
        if closing {
            depth = depth.saturating_sub(1);
            continue;
        }
        elements = elements.saturating_add(1);
        if elements > MAX_SVG_ELEMENTS {
            return Err(format!(
                "SVG exceeds the {MAX_SVG_ELEMENTS} element safety limit"
            ));
        }
        if !self_closing {
            depth = depth.saturating_add(1);
            if depth > MAX_SVG_DEPTH {
                return Err(format!(
                    "SVG exceeds the {MAX_SVG_DEPTH} level nesting safety limit"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_preflight_rejects_entity_expansion_documents() {
        let source = br#"<!DOCTYPE svg [<!ENTITY a "aaaaaaaa">]><svg>&a;</svg>"#;
        assert_eq!(
            validate_svg_source(source).unwrap_err(),
            "SVG document types are not supported"
        );
    }

    #[test]
    fn svg_preflight_rejects_pathological_nesting() {
        let source = format!("<svg>{}</svg>", "<g>".repeat(MAX_SVG_DEPTH + 1));
        assert!(
            validate_svg_source(source.as_bytes())
                .unwrap_err()
                .contains("nesting safety limit")
        );
    }

    #[test]
    fn svg_preflight_accepts_normal_markup() {
        validate_svg_source(br#"<?xml version="1.0"?><svg><g><rect /></g></svg>"#).unwrap();
    }
}
