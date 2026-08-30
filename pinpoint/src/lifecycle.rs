use crate::stage::Stage;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

const RELOAD_DEBOUNCE: Duration = Duration::from_millis(200);
const READINESS_POLL: Duration = Duration::from_millis(25);

pub struct Lifecycle {
    stage: glib::WeakRef<Stage>,
    view: glib::WeakRef<gtk::Stack>,
    path: PathBuf,
    ignore_comments: bool,
    asset_access: pinpoint_core::asset::Access,
    read_only: bool,
    source_changed: Box<dyn Fn()>,
    source_monitor: RefCell<Option<gio::FileMonitor>>,
    asset_monitors: RefCell<Vec<gio::FileMonitor>>,
    reload_source: RefCell<Option<glib::SourceId>>,
    readiness_source: RefCell<Option<glib::SourceId>>,
    ready_generation: Cell<u64>,
    reloads: Cell<u64>,
    asset_invalidations: Cell<u64>,
    last_error: RefCell<Option<String>>,
}

impl Lifecycle {
    pub fn start(
        stage: &Stage,
        view: &gtk::Stack,
        path: PathBuf,
        ignore_comments: bool,
        asset_access: pinpoint_core::asset::Access,
        source_changed: impl Fn() + 'static,
    ) -> Rc<Self> {
        let read_only = path.starts_with("/app/share/pinpoint");
        let lifecycle = Rc::new(Self {
            stage: stage.downgrade(),
            view: view.downgrade(),
            path,
            ignore_comments,
            asset_access,
            read_only,
            source_changed: Box::new(source_changed),
            source_monitor: RefCell::new(None),
            asset_monitors: RefCell::new(Vec::new()),
            reload_source: RefCell::new(None),
            readiness_source: RefCell::new(None),
            ready_generation: Cell::new(0),
            reloads: Cell::new(0),
            asset_invalidations: Cell::new(0),
            last_error: RefCell::new(None),
        });
        lifecycle.install_monitors();
        lifecycle.start_readiness_poll();
        lifecycle
    }

    pub fn reload_count(&self) -> u64 {
        self.reloads.get()
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error.borrow().clone()
    }

    pub fn asset_invalidation_count(&self) -> u64 {
        self.asset_invalidations.get()
    }

    fn file_matches(path: &Path, file: &gio::File) -> bool {
        file.path().as_deref() == Some(path)
    }

    fn install_monitors(self: &Rc<Self>) {
        if let Some(monitor) = self.source_monitor.borrow_mut().take() {
            monitor.cancel();
        }
        for monitor in self.asset_monitors.borrow_mut().drain(..) {
            monitor.cancel();
        }
        if self.read_only {
            return;
        }

        let file = gio::File::for_path(&self.path);
        let source_monitor = file
            .parent()
            .and_then(|parent| {
                parent
                    .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
                    .ok()
            })
            .or_else(|| {
                file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
                    .ok()
            });
        if let Some(monitor) = source_monitor {
            monitor.set_rate_limit(100);
            let weak = Rc::downgrade(self);
            monitor.connect_changed(move |_, file, other, _| {
                let Some(lifecycle) = weak.upgrade() else {
                    return;
                };
                if Self::file_matches(&lifecycle.path, file)
                    || other.is_some_and(|other| Self::file_matches(&lifecycle.path, other))
                {
                    (lifecycle.source_changed)();
                    lifecycle.schedule_reload();
                }
            });
            *self.source_monitor.borrow_mut() = Some(monitor);
        } else {
            eprintln!(
                "PINPOINT LIFECYCLE monitor-failed path={}",
                self.path.display()
            );
        }

        let Some(stage) = self.stage.upgrade() else {
            return;
        };
        for path in stage.monitored_assets() {
            let file = gio::File::for_path(&path);
            let Ok(monitor) =
                file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            else {
                eprintln!(
                    "PINPOINT LIFECYCLE asset-monitor-failed path={}",
                    path.display()
                );
                continue;
            };
            monitor.set_rate_limit(100);
            let weak = Rc::downgrade(self);
            monitor.connect_changed(move |_, file, other, _| {
                let Some(lifecycle) = weak.upgrade() else {
                    return;
                };
                let Some(stage) = lifecycle.stage.upgrade() else {
                    return;
                };
                if let Some(path) = file.path() {
                    stage.invalidate_asset(&path);
                    lifecycle
                        .asset_invalidations
                        .set(lifecycle.asset_invalidations.get().saturating_add(1));
                    eprintln!(
                        "PINPOINT LIFECYCLE asset-invalidated path={}",
                        path.display()
                    );
                }
                if let Some(path) = other.and_then(gio::File::path) {
                    stage.invalidate_asset(&path);
                    lifecycle
                        .asset_invalidations
                        .set(lifecycle.asset_invalidations.get().saturating_add(1));
                    eprintln!(
                        "PINPOINT LIFECYCLE asset-invalidated path={}",
                        path.display()
                    );
                }
            });
            self.asset_monitors.borrow_mut().push(monitor);
        }
    }

    fn schedule_reload(self: &Rc<Self>) {
        if let Some(source) = self.reload_source.borrow_mut().take() {
            source.remove();
        }
        if let Some(view) = self.view.upgrade() {
            view.set_visible_child_name("loading");
        }
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(RELOAD_DEBOUNCE, move || {
            let Some(lifecycle) = weak.upgrade() else {
                return;
            };
            lifecycle.reload_source.borrow_mut().take();
            lifecycle.reload();
        });
        *self.reload_source.borrow_mut() = Some(source);
    }

    fn reload(self: &Rc<Self>) {
        let Some(stage) = self.stage.upgrade() else {
            return;
        };
        match pinpoint_core::presentation::load(&self.path, self.ignore_comments) {
            Ok(mut presentation) => {
                presentation.asset_access = self.asset_access;
                let slide = stage.reload_presentation(presentation, self.path.clone());
                self.reloads.set(self.reloads.get().saturating_add(1));
                self.last_error.borrow_mut().take();
                eprintln!(
                    "PINPOINT LIFECYCLE reloaded count={} selected_slide={} path={}",
                    self.reloads.get(),
                    slide,
                    self.path.display()
                );
                self.install_monitors();
            }
            Err(error) => {
                let error = error.to_string();
                eprintln!("PINPOINT LIFECYCLE reload-failed: {error}");
                *self.last_error.borrow_mut() = Some(error);
                if let Some(view) = self.view.upgrade() {
                    view.set_visible_child_name("stage");
                }
            }
        }
    }

    fn start_readiness_poll(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local(READINESS_POLL, move || {
            let Some(lifecycle) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let Some(stage) = lifecycle.stage.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let generation = stage.generation();
            if stage.is_ready() {
                if let Some(view) = lifecycle.view.upgrade()
                    && view.visible_child_name().as_deref() != Some("stage")
                {
                    view.set_visible_child_name("stage");
                }
                if lifecycle.ready_generation.get() != generation {
                    lifecycle.ready_generation.set(generation);
                    eprintln!(
                        "PINPOINT LIFECYCLE ready generation={generation} slide={}",
                        stage.current_slide()
                    );
                }
            } else if let Some(view) = lifecycle.view.upgrade() {
                view.set_visible_child_name("loading");
            }
            glib::ControlFlow::Continue
        });
        *self.readiness_source.borrow_mut() = Some(source);
    }

    pub fn stop(&self) {
        if let Some(source) = self.reload_source.borrow_mut().take() {
            source.remove();
        }
        if let Some(source) = self.readiness_source.borrow_mut().take() {
            source.remove();
        }
        if let Some(monitor) = self.source_monitor.borrow_mut().take() {
            monitor.cancel();
        }
        for monitor in self.asset_monitors.borrow_mut().drain(..) {
            monitor.cancel();
        }
    }
}
