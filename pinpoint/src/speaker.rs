use crate::asset_store::AssetStore;
use crate::command_runner::CommandRunner;
use crate::mpris;
use crate::presentation_chrome::PresentationChrome;
use crate::stage::{PixelAgreement, Stage};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use pinpoint_core::command;
use pinpoint_core::display_selection::{
    DisplayCandidate, DisplaySelection, choose as choose_displays,
};
use pinpoint_core::presentation::Presentation;
use pinpoint_core::rehearsal::{Rehearsal, finish_to_path};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct SpeakerPresenter(Rc<Inner>);

struct Inner {
    audience_window: adw::ApplicationWindow,
    speaker_window: adw::ApplicationWindow,
    speaker_shell: adw::ToolbarView,
    speaker_toolbar: gtk::Revealer,
    audience: Stage,
    audience_chrome: PresentationChrome,
    previous: Stage,
    current: Stage,
    next: Stage,
    preview_scroll: gtk::ScrolledWindow,
    presentation: Rc<Presentation>,
    assets: AssetStore,
    notes: gtk::Label,
    position: gtk::Label,
    remaining: gtk::Label,
    progress: gtk::ProgressBar,
    source_path: PathBuf,
    source_snapshot: RefCell<Option<String>>,
    apply_timings: Option<Rc<dyn Fn(Vec<f64>)>>,
    rehearsal: RefCell<Option<ActiveRehearsal>>,
    rehearsal_error: RefCell<Option<String>>,
    timer_started: RefCell<Option<Instant>>,
    timer_elapsed: Cell<Duration>,
    timer_source: RefCell<Option<glib::SourceId>>,
    slide_started: Cell<Duration>,
    autoadvance: Cell<bool>,
    fullscreen: Cell<bool>,
    monitor_model: gio::ListModel,
    monitors: RefCell<RuntimeSelection>,
    mpris: RefCell<Option<mpris::Mpris>>,
    command_runner: CommandRunner,
    command_trust: RefCell<command::Trust>,
    source_monitor: RefCell<Option<gio::FileMonitor>>,
    command_notice: gtk::Revealer,
    command_notice_label: gtk::Label,
    command_notice_control: RefCell<Option<glib::SourceId>>,
    closing: Cell<bool>,
}

struct ActiveRehearsal {
    timings: Rehearsal,
    timed_slide: usize,
    slide_started: Duration,
}

#[derive(Clone, Default)]
struct RuntimeSelection {
    presenter: Option<gdk::Monitor>,
    audience: Option<gdk::Monitor>,
}

fn monitor_is_builtin(monitor: &gdk::Monitor) -> bool {
    monitor.connector().is_some_and(|connector| {
        ["eDP", "LVDS", "DSI"]
            .iter()
            .any(|prefix| connector.starts_with(prefix))
    })
}

fn monitor_name(monitor: &gdk::Monitor) -> String {
    monitor
        .connector()
        .or_else(|| monitor.model())
        .map_or_else(|| "unknown".to_owned(), |name| name.to_string())
}

fn model_monitors(model: &gio::ListModel) -> Vec<gdk::Monitor> {
    (0..model.n_items())
        .filter_map(|index| model.item(index)?.downcast::<gdk::Monitor>().ok())
        .filter(gdk::Monitor::is_valid)
        .collect()
}

fn monitor_index(monitors: &[gdk::Monitor], selected: Option<&gdk::Monitor>) -> Option<u32> {
    selected.and_then(|selected| {
        monitors
            .iter()
            .position(|monitor| monitor == selected)
            .map(|index| index as u32)
    })
}

fn preview_box(stage: &Stage, width: i32, height: i32) -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
    container.add_css_class("pinpoint-speaker-preview");
    stage.set_size_request(width, height);
    stage.set_hexpand(true);
    stage.set_vexpand(true);
    container.append(stage);
    container
}

fn install_speaker_css(display: &gdk::Display) {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        "window.pinpoint-speaker { background: #000; }\
         .pinpoint-speaker-content { background: #000; }\
         .pinpoint-speaker-preview-scroll, .pinpoint-speaker-preview-scroll viewport, .pinpoint-speaker-preview-rail { background: #ddd; }\
         .pinpoint-speaker-preview { background: #ddd; }\
         .pinpoint-speaker-notes, .pinpoint-speaker-notes viewport { background: transparent; color: #fff; }\
         .pinpoint-speaker-notes-label { color: #fff; }\
         .pinpoint-speaker-toolbar { background: rgba(36,36,36,0.94); padding: 6px; border-radius: 12px; box-shadow: 0 2px 8px rgba(0,0,0,0.45); }\
         .pinpoint-speaker-toolbar button { min-height: 32px; min-width: 32px; padding: 2px 9px; color: #fff; }\
         .pinpoint-speaker-toolbar button.flat:hover { background: rgba(255,255,255,0.14); }\
         .pinpoint-speaker-toolbar button:checked { background: rgba(255,255,255,0.22); }\
         .pinpoint-speaker-position { color: #fff; font: 18px Sans; }\
         .pinpoint-speaker-remaining { color: #fff; font: 24px Sans; }",
    );
    gtk::style_context_add_provider_for_display(
        display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn format_time(seconds: f64) -> String {
    let seconds = seconds.round() as i64;
    let sign = if seconds < 0 { "−" } else { "" };
    let seconds = seconds.unsigned_abs();
    format!("{sign}{}:{:02}", seconds / 60, seconds % 60)
}

impl SpeakerPresenter {
    pub fn new(
        app: &adw::Application,
        presentation: Presentation,
        path: PathBuf,
        presentation_label: String,
        fullscreen: bool,
    ) -> Self {
        Self::new_with_rehearsal(
            app,
            presentation,
            path.clone(),
            presentation_label,
            fullscreen,
            std::fs::read_to_string(path).ok(),
            None,
        )
    }

    pub fn new_for_editor(
        app: &adw::Application,
        presentation: Presentation,
        path: PathBuf,
        source_snapshot: String,
        apply_timings: impl Fn(Vec<f64>) + 'static,
    ) -> Self {
        Self::new_with_rehearsal(
            app,
            presentation,
            path,
            "Presentation".into(),
            false,
            Some(source_snapshot),
            Some(Rc::new(apply_timings)),
        )
    }

    fn new_with_rehearsal(
        app: &adw::Application,
        presentation: Presentation,
        path: PathBuf,
        presentation_label: String,
        fullscreen: bool,
        source_snapshot: Option<String>,
        apply_timings: Option<Rc<dyn Fn(Vec<f64>)>>,
    ) -> Self {
        let monitor_source = apply_timings.is_none();
        let presentation = Rc::new(presentation);
        let assets = AssetStore::default();
        let audience = Stage::with_shared_assets(assets.clone(), false);
        let previous = Stage::with_shared_assets(assets.clone(), true);
        let current = Stage::with_shared_assets(assets.clone(), true);
        let next = Stage::with_shared_assets(assets.clone(), true);
        previous.set_accessible_context("Previous slide");
        current.set_accessible_context("Current slide");
        next.set_accessible_context("Next slide");
        for (stage, slide) in [(&audience, 0), (&previous, 0), (&current, 0), (&next, 1)] {
            stage.set_shared_presentation_at(presentation.clone(), path.clone(), slide);
        }

        let audience_window = adw::ApplicationWindow::builder()
            .application(app)
            .title(format!("{presentation_label} — Pinpoint Audience"))
            .default_width(1280)
            .default_height(720)
            .build();
        let (audience_content, audience_chrome) =
            PresentationChrome::new(&audience_window, &audience, &audience);
        audience_window.set_content(Some(&audience_content));

        let notes = gtk::Label::builder()
            .label("")
            .wrap(true)
            .xalign(0.0)
            .yalign(0.0)
            .hexpand(true)
            .vexpand(true)
            .build();
        notes.set_accessible_role(gtk::AccessibleRole::Label);
        notes.update_property(&[gtk::accessible::Property::Label("Speaker notes")]);
        notes.set_tooltip_text(Some("Speaker notes"));
        let notes_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&notes)
            .build();
        notes_scroll.add_css_class("pinpoint-speaker-notes");
        notes.add_css_class("pinpoint-speaker-notes-label");

        let previews = gtk::Box::new(gtk::Orientation::Vertical, 2);
        previews.add_css_class("pinpoint-speaker-preview-rail");
        previews.set_size_request(420, 712);
        previews.set_hexpand(false);
        previews.set_vexpand(false);
        previews.append(&preview_box(&previous, 420, 236));
        previews.append(&preview_box(&current, 420, 236));
        previews.append(&preview_box(&next, 420, 236));

        let preview_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .kinetic_scrolling(true)
            .overlay_scrolling(true)
            .min_content_width(420)
            .vexpand(true)
            .child(&previews)
            .build();
        preview_scroll.add_css_class("pinpoint-speaker-preview-scroll");
        preview_scroll.update_property(&[gtk::accessible::Property::Label(
            "Previous, current, and next slide previews",
        )]);
        preview_scroll.set_tooltip_text(Some("Scroll vertically to review the surrounding slides"));

        let body = gtk::Paned::new(gtk::Orientation::Horizontal);
        body.set_start_child(Some(&preview_scroll));
        body.set_end_child(Some(&notes_scroll));
        body.set_resize_start_child(false);
        body.set_resize_end_child(true);
        body.set_shrink_start_child(false);
        body.set_shrink_end_child(false);
        body.set_vexpand(true);

        let position = gtk::Label::new(None);
        position.add_css_class("pinpoint-speaker-position");
        position.update_property(&[gtk::accessible::Property::Label("Current slide position")]);
        let remaining = gtk::Label::new(Some("0:00"));
        remaining.add_css_class("title-2");
        remaining.update_property(&[gtk::accessible::Property::Label(
            "Remaining presentation time",
        )]);
        let progress = gtk::ProgressBar::new();
        progress.set_hexpand(true);
        progress.update_property(&[
            gtk::accessible::Property::Label("Presentation timing progress"),
            gtk::accessible::Property::Description(
                "Visual comparison of elapsed time and the current slide plan",
            ),
        ]);

        let hide_speaker_button = gtk::Button::from_icon_name("view-conceal-symbolic");
        hide_speaker_button.set_tooltip_text(Some("Hide Speaker View"));
        hide_speaker_button
            .update_property(&[gtk::accessible::Property::Label("Hide Speaker View")]);
        let start_button = gtk::Button::from_icon_name("media-playback-start-symbolic");
        start_button.set_tooltip_text(Some("Restart Presentation Timer"));
        start_button.update_property(&[gtk::accessible::Property::Label(
            "Restart Presentation Timer",
        )]);
        let pause_button = gtk::Button::from_icon_name("media-playback-pause-symbolic");
        pause_button.set_tooltip_text(Some("Pause or Resume Presentation Timer"));
        pause_button.update_property(&[gtk::accessible::Property::Label(
            "Pause or Resume Presentation Timer",
        )]);
        let rehearse_button = gtk::Button::with_label("Rehearse");
        let autoadvance_button = gtk::ToggleButton::with_label("Autoadvance");
        let swap_button = gtk::Button::from_icon_name("view-refresh-symbolic");
        swap_button.set_tooltip_text(Some("Swap the audience and speaker displays"));
        swap_button.update_property(&[
            gtk::accessible::Property::Label("Swap Displays"),
            gtk::accessible::Property::KeyShortcuts("S"),
        ]);
        let fullscreen_button = gtk::Button::from_icon_name("view-fullscreen-symbolic");
        fullscreen_button.set_tooltip_text(Some("Toggle Speaker View Fullscreen"));
        fullscreen_button.update_property(&[
            gtk::accessible::Property::Label("Toggle Speaker View Fullscreen"),
            gtk::accessible::Property::KeyShortcuts("F11"),
        ]);
        for button in [
            &hide_speaker_button,
            &start_button,
            &pause_button,
            &rehearse_button,
            &swap_button,
            &fullscreen_button,
        ] {
            button.add_css_class("flat");
        }
        autoadvance_button.add_css_class("flat");

        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        toolbar.set_halign(gtk::Align::Center);
        toolbar.set_valign(gtk::Align::Start);
        toolbar.set_margin_top(12);
        toolbar.add_css_class("pinpoint-speaker-toolbar");
        toolbar.append(&hide_speaker_button);
        toolbar.append(&start_button);
        toolbar.append(&pause_button);
        toolbar.append(&autoadvance_button);
        toolbar.append(&rehearse_button);
        toolbar.append(&swap_button);
        toolbar.append(&fullscreen_button);
        let speaker_toolbar = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .transition_duration(100)
            .reveal_child(true)
            .child(&toolbar)
            .build();

        let timing = gtk::Overlay::new();
        timing.set_halign(gtk::Align::Fill);
        timing.set_valign(gtk::Align::End);
        timing.set_size_request(-1, 32);
        timing.set_child(Some(&progress));
        remaining.set_halign(gtk::Align::End);
        remaining.set_valign(gtk::Align::End);
        remaining.set_margin_end(4);
        remaining.set_margin_bottom(4);
        remaining.remove_css_class("title-2");
        remaining.add_css_class("pinpoint-speaker-remaining");
        timing.add_overlay(&remaining);
        position.set_halign(gtk::Align::Start);
        position.set_valign(gtk::Align::End);
        position.set_margin_start(6);
        position.set_margin_bottom(5);
        timing.add_overlay(&position);

        let speaker_content = gtk::Overlay::new();
        speaker_content.add_css_class("pinpoint-speaker-content");
        speaker_content.set_child(Some(&body));
        speaker_content.add_overlay(&speaker_toolbar);
        speaker_content.add_overlay(&timing);
        let command_notice_label = gtk::Label::new(None);
        command_notice_label.add_css_class("osd");
        command_notice_label.set_margin_start(14);
        command_notice_label.set_margin_end(14);
        command_notice_label.set_margin_top(8);
        command_notice_label.set_margin_bottom(8);
        let command_notice = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .transition_duration(200)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .margin_bottom(48)
            .child(&command_notice_label)
            .build();
        speaker_content.add_overlay(&command_notice);
        let speaker_header = adw::HeaderBar::builder()
            .title_widget(&adw::WindowTitle::new("Speaker View", ""))
            .build();
        let speaker_shell = adw::ToolbarView::new();
        speaker_shell.add_top_bar(&speaker_header);
        speaker_shell.set_content(Some(&speaker_content));
        let speaker_window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Pinpoint Speaker View")
            .default_width(1000)
            .default_height(700)
            .content(&speaker_shell)
            .build();
        speaker_window.add_css_class("pinpoint-speaker");
        install_speaker_css(&gtk::prelude::WidgetExt::display(&speaker_window));
        speaker_window.connect_fullscreened_notify(glib::clone!(
            #[weak]
            speaker_shell,
            move |window| speaker_shell.set_reveal_top_bars(!window.is_fullscreen())
        ));

        let monitor_model = gtk::prelude::WidgetExt::display(&audience_window).monitors();
        rehearse_button.set_sensitive(source_snapshot.is_some());
        rehearse_button.set_tooltip_text(Some(
            "Record slide timings and apply them when the final slide advances",
        ));
        let mpris_state = Rc::new(RefCell::new(mpris::State {
            presenting: true,
            slide: 0,
            slides: presentation.slides.len(),
            fullscreen,
        }));
        let presenter = Self(Rc::new(Inner {
            audience_window,
            speaker_window,
            speaker_shell,
            speaker_toolbar,
            audience,
            audience_chrome,
            previous,
            current,
            next,
            preview_scroll,
            presentation,
            assets,
            notes,
            position,
            remaining,
            progress,
            source_path: path.clone(),
            source_snapshot: RefCell::new(source_snapshot),
            apply_timings,
            rehearsal: RefCell::new(None),
            rehearsal_error: RefCell::new(None),
            timer_started: RefCell::new(None),
            timer_elapsed: Cell::new(Duration::ZERO),
            timer_source: RefCell::new(None),
            slide_started: Cell::new(Duration::ZERO),
            autoadvance: Cell::new(false),
            fullscreen: Cell::new(fullscreen),
            monitor_model,
            monitors: RefCell::new(RuntimeSelection::default()),
            mpris: RefCell::new(None),
            command_runner: CommandRunner::default(),
            command_trust: RefCell::new(command::Trust::default()),
            source_monitor: RefCell::new(None),
            command_notice,
            command_notice_label,
            command_notice_control: RefCell::new(None),
            closing: Cell::new(false),
        }));

        let weak = Rc::downgrade(&presenter.0);
        hide_speaker_button.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                Self(inner).0.speaker_window.set_visible(false);
            }
        });
        let weak = Rc::downgrade(&presenter.0);
        start_button.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                Self(inner).start_timer();
            }
        });
        let weak = Rc::downgrade(&presenter.0);
        pause_button.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                Self(inner).toggle_timer();
            }
        });
        let weak = Rc::downgrade(&presenter.0);
        autoadvance_button.connect_toggled(move |button| {
            if let Some(inner) = weak.upgrade() {
                Self(inner).set_autoadvance(button.is_active());
            }
        });
        let weak = Rc::downgrade(&presenter.0);
        rehearse_button.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade()
                && let Err(error) = Self(inner).start_rehearsal()
            {
                eprintln!("pinpoint: unable to start rehearsal: {error}");
            }
        });
        let weak = Rc::downgrade(&presenter.0);
        swap_button.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                Self(inner).swap_displays();
            }
        });
        let weak = Rc::downgrade(&presenter.0);
        fullscreen_button.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                Self(inner).toggle_fullscreen();
            }
        });
        presenter.install_keys(&presenter.0.audience_window);
        presenter.install_keys(&presenter.0.speaker_window);
        presenter.install_pointer_controls();
        if monitor_source {
            presenter.install_source_monitor();
        }

        let weak = Rc::downgrade(&presenter.0);
        presenter
            .0
            .monitor_model
            .connect_items_changed(move |_, _, _, _| {
                if let Some(inner) = weak.upgrade() {
                    Self(inner).apply_monitor_policy();
                }
            });
        let weak = Rc::downgrade(&presenter.0);
        presenter.0.audience_window.connect_close_request(move |_| {
            if let Some(inner) = weak.upgrade() {
                let presenter = Self(inner);
                if !presenter.0.closing.replace(true) {
                    presenter.stop();
                    presenter.0.speaker_window.close();
                }
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(&presenter.0);
        presenter.0.speaker_window.connect_close_request(move |_| {
            if let Some(inner) = weak.upgrade() {
                let presenter = Self(inner);
                if !presenter.0.closing.replace(true) {
                    presenter.stop();
                    presenter.0.audience_window.close();
                }
            }
            glib::Propagation::Proceed
        });

        let weak = Rc::downgrade(&presenter.0);
        match mpris::Mpris::new(app, mpris_state, move |command| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let presenter = Self(inner);
            match command {
                mpris::Command::Next => {
                    presenter.next();
                }
                mpris::Command::Previous => {
                    presenter.previous();
                }
                mpris::Command::Fullscreen(fullscreen) => {
                    if presenter.0.fullscreen.get() != fullscreen {
                        presenter.toggle_fullscreen();
                    }
                }
            }
        }) {
            Ok(mpris) => *presenter.0.mpris.borrow_mut() = Some(mpris),
            Err(error) => eprintln!("pinpoint: unable to export MPRIS controls: {error}"),
        }

        presenter.update_views();
        presenter.start_timer_updates();
        presenter.0.audience_window.present();
        presenter.0.speaker_window.present();
        presenter.apply_monitor_policy();
        presenter.sync_mpris();
        presenter
    }

    fn install_keys(&self, window: &adw::ApplicationWindow) {
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&self.0);
        let command_parent = window.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(inner) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let presenter = Self(inner);
            let handled = match key {
                gdk::Key::Right
                | gdk::Key::Down
                | gdk::Key::space
                | gdk::Key::Page_Down
                | gdk::Key::AudioNext
                | gdk::Key::Forward => presenter.next(),
                gdk::Key::Left
                | gdk::Key::Up
                | gdk::Key::BackSpace
                | gdk::Key::Page_Up
                | gdk::Key::AudioPrev
                | gdk::Key::Back => presenter.previous(),
                gdk::Key::Home | gdk::Key::h | gdk::Key::H => presenter.first(),
                gdk::Key::End => presenter.last(),
                gdk::Key::b | gdk::Key::B => {
                    presenter.0.audience.toggle_blank();
                    true
                }
                gdk::Key::c | gdk::Key::C => presenter.0.audience.retry_camera(),
                gdk::Key::s | gdk::Key::S => presenter.swap_displays(),
                gdk::Key::F11 | gdk::Key::f | gdk::Key::F => {
                    presenter.toggle_fullscreen();
                    true
                }
                gdk::Key::F1 => {
                    presenter.toggle_speaker_visibility();
                    true
                }
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    if let Err(error) = presenter.request_current_command(&command_parent) {
                        presenter.show_command_notice(&error);
                    }
                    true
                }
                gdk::Key::Escape | gdk::Key::q | gdk::Key::Q => {
                    command_parent.close();
                    true
                }
                _ => false,
            };
            if handled {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        window.add_controller(keys);
    }

    fn install_pointer_controls(&self) {
        let click = gtk::GestureClick::new();
        let weak = Rc::downgrade(&self.0);
        click.connect_released(move |gesture, _, _, _| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let presenter = Self(inner);
            match gesture.current_button() {
                1 => {
                    presenter.next();
                }
                3 => {
                    presenter.previous();
                }
                _ => return,
            }
            presenter.0.audience_chrome.hide();
        });
        self.0.audience.add_controller(click);
    }

    fn set_preview(stage: &Stage, slide: Option<usize>) {
        stage.set_visible(slide.is_some());
        if let Some(slide) = slide {
            stage.set_slide_without_transition(slide);
        }
    }

    fn update_views(&self) {
        let current = self.0.audience.current_slide();
        Self::set_preview(&self.0.previous, current.checked_sub(1));
        Self::set_preview(&self.0.current, Some(current));
        Self::set_preview(
            &self.0.next,
            (current + 1 < self.0.presentation.slides.len()).then_some(current + 1),
        );
        let slide = &self.0.presentation.slides[current];
        self.0
            .notes
            .set_label(slide.speaker_notes.as_deref().unwrap_or(""));
        self.0.position.set_label(&format!(
            "Slide {} of {}",
            current + 1,
            self.0.presentation.slides.len()
        ));
        self.update_timer();
        self.sync_mpris();
    }

    pub fn next(&self) -> bool {
        let old_slide = self.current_slide();
        let changed = self.0.audience.next();
        if changed {
            self.record_rehearsal_slide(old_slide);
            self.begin_rehearsal_slide(self.current_slide());
            self.0.audience_chrome.hide();
            self.update_views();
        } else if self.rehearsal_active() {
            self.finish_rehearsal();
        } else if self.0.autoadvance.replace(false) {
            self.update_timer();
        }
        changed
    }

    pub fn previous(&self) -> bool {
        let old_slide = self.current_slide();
        let changed = self.0.audience.previous();
        if changed {
            self.record_rehearsal_slide(old_slide);
            self.begin_rehearsal_slide(self.current_slide());
            self.0.slide_started.set(self.elapsed());
            self.0.audience_chrome.hide();
            self.update_views();
        }
        changed
    }

    pub fn first(&self) -> bool {
        let old_slide = self.current_slide();
        let changed = self.0.audience.first();
        if changed {
            self.record_rehearsal_slide(old_slide);
            self.begin_rehearsal_slide(self.current_slide());
            self.0.slide_started.set(self.elapsed());
            self.0.audience_chrome.hide();
            self.update_views();
        }
        changed
    }

    pub fn last(&self) -> bool {
        let old_slide = self.current_slide();
        let changed = self.0.audience.last();
        if changed {
            self.record_rehearsal_slide(old_slide);
            self.begin_rehearsal_slide(self.current_slide());
            self.0.slide_started.set(self.elapsed());
            self.0.audience_chrome.hide();
            self.update_views();
        }
        changed
    }

    pub fn current_slide(&self) -> usize {
        self.0.audience.current_slide()
    }

    pub fn notes(&self) -> String {
        self.0.notes.label().to_string()
    }

    pub fn previews_match(&self) -> bool {
        let current = self.current_slide();
        self.0.current.current_slide() == current
            && (current == 0 || self.0.previous.current_slide() == current - 1)
            && (current + 1 >= self.0.presentation.slides.len()
                || self.0.next.current_slide() == current + 1)
    }

    pub fn preview_scroll_matches_contract(&self) -> bool {
        if self.0.preview_scroll.hscrollbar_policy() != gtk::PolicyType::Never
            || self.0.preview_scroll.vscrollbar_policy() != gtk::PolicyType::Automatic
            || !self.0.preview_scroll.is_kinetic_scrolling()
        {
            return false;
        }

        let slide_before = self.current_slide();
        let adjustment = self.0.preview_scroll.vadjustment();
        let original = adjustment.value();
        let maximum = (adjustment.upper() - adjustment.page_size()).max(0.0);
        if maximum <= 0.0 {
            return false;
        }
        adjustment.set_value(maximum);
        let scrolled = adjustment.value() > original;
        adjustment.set_value(original);
        scrolled && self.current_slide() == slide_before
    }

    pub fn pixel_agreement(&self) -> Result<PixelAgreement, String> {
        if self.0.audience.current_slide() != self.0.current.current_slide() {
            return Err("audience and speaker current preview selected different slides".into());
        }
        self.0
            .audience
            .pixel_agreement_with(&self.0.current, 640, 360)
    }

    pub fn audience_is_transitioning(&self) -> bool {
        self.0.audience.is_transitioning()
    }

    pub fn assets_are_shared(&self) -> bool {
        [&self.0.previous, &self.0.current, &self.0.next]
            .iter()
            .all(|stage| self.0.audience.shares_assets_with(stage))
    }

    pub fn presentation_chrome_matches_contract(&self) -> bool {
        if self.0.audience_window.is_decorated()
            || !self.0.speaker_window.is_decorated()
            || (!self.0.speaker_window.is_fullscreen() && !self.0.speaker_shell.reveals_top_bars())
            || (!self.0.speaker_window.is_fullscreen() && !self.0.speaker_toolbar.reveals_child())
        {
            return false;
        }
        self.0.audience_chrome.reveal_for_input();
        let revealed = self.0.audience_chrome.is_revealed();
        self.0.audience_chrome.hide();
        revealed
    }

    pub fn speaker_visibility_matches_contract(&self) -> bool {
        self.toggle_speaker_visibility();
        let hidden = !self.0.speaker_window.is_visible() && self.0.audience_window.is_visible();
        self.toggle_speaker_visibility();
        hidden && self.0.speaker_window.is_visible()
    }

    pub fn asset_stats(&self) -> crate::asset_store::Stats {
        self.0.assets.stats()
    }

    pub fn monitor_count(&self) -> u32 {
        self.0.monitor_model.n_items()
    }

    pub fn close_speaker_for_validation(&self) {
        self.0.speaker_window.close();
    }

    pub fn paired_windows_are_closed(&self) -> bool {
        !self.0.speaker_window.is_visible() && !self.0.audience_window.is_visible()
    }

    pub fn close(&self) {
        self.0.audience_window.close();
    }

    fn toggle_speaker_visibility(&self) {
        if self.0.speaker_window.is_visible() {
            self.0.speaker_window.set_visible(false);
        } else {
            self.0.speaker_window.present();
        }
    }

    pub fn prefer_audience_monitor(&self, name: &str) -> bool {
        let monitors = model_monitors(&self.0.monitor_model);
        let Some(audience) = monitors
            .iter()
            .find(|monitor| monitor_name(monitor) == name)
            .cloned()
        else {
            return false;
        };
        let presenter = monitors
            .iter()
            .find(|monitor| *monitor != &audience && monitor_is_builtin(monitor))
            .or_else(|| monitors.iter().find(|monitor| *monitor != &audience))
            .cloned();
        *self.0.monitors.borrow_mut() = RuntimeSelection {
            presenter,
            audience: Some(audience),
        };
        if self.0.fullscreen.get() {
            self.reenter_fullscreen_on_selected_monitors();
        }
        true
    }

    fn elapsed(&self) -> Duration {
        self.0.timer_elapsed.get()
            + self
                .0
                .timer_started
                .borrow()
                .as_ref()
                .map_or(Duration::ZERO, Instant::elapsed)
    }

    fn total_duration(&self) -> f64 {
        self.0
            .presentation
            .slides
            .iter()
            .map(|slide| slide.duration.max(2.0))
            .sum()
    }

    fn update_timer(&self) {
        let elapsed = self.elapsed().as_secs_f64();
        let total = self.total_duration();
        self.0.remaining.set_label(&format_time(total - elapsed));
        self.0
            .progress
            .set_fraction(if total > 0.0 { elapsed / total } else { 0.0 });
    }

    fn start_timer_updates(&self) {
        let weak: Weak<Inner> = Rc::downgrade(&self.0);
        let source = glib::timeout_add_local(Duration::from_millis(50), move || {
            let Some(inner) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let presenter = Self(inner);
            presenter.update_timer();
            presenter.maybe_autoadvance();
            glib::ControlFlow::Continue
        });
        *self.0.timer_source.borrow_mut() = Some(source);
    }

    pub fn start_timer(&self) {
        self.cancel_rehearsal();
        self.0.audience.first();
        self.0.timer_elapsed.set(Duration::ZERO);
        *self.0.timer_started.borrow_mut() = Some(Instant::now());
        self.0.slide_started.set(Duration::ZERO);
        self.update_views();
        self.update_timer();
    }

    pub fn toggle_timer(&self) {
        if let Some(started) = self.0.timer_started.borrow_mut().take() {
            self.0
                .timer_elapsed
                .set(self.0.timer_elapsed.get() + started.elapsed());
        } else {
            *self.0.timer_started.borrow_mut() = Some(Instant::now());
        }
        self.update_timer();
    }

    pub fn set_autoadvance(&self, active: bool) {
        self.0.autoadvance.set(active);
    }

    pub fn autoadvance(&self) -> bool {
        self.0.autoadvance.get()
    }

    pub fn run_current_command(&self) -> Result<(), String> {
        let command = command::allow(
            true,
            self.0.presentation.slides[self.current_slide()]
                .command
                .as_deref(),
        )
        .map_err(|error| error.to_string())?;
        self.show_command_notice("Running slide command…");
        let weak = Rc::downgrade(&self.0);
        self.0
            .command_runner
            .run_with_callback(command, move |result| {
                if let Some(inner) = weak.upgrade() {
                    Self(inner).show_command_notice(&match result {
                        Ok(()) => "Slide command finished".into(),
                        Err(error) => format!("Slide command failed — {error}"),
                    });
                }
            })
    }

    fn request_current_command(&self, window: &adw::ApplicationWindow) -> Result<(), String> {
        const REVISION: u64 = 1;
        if self.0.command_runner.is_running() {
            self.0.command_runner.cancel("user-requested");
            self.show_command_notice("Slide command stopped");
            return Ok(());
        }
        let current_command = self.0.presentation.slides[self.current_slide()]
            .command
            .as_deref();
        match self
            .0
            .command_trust
            .borrow_mut()
            .request(true, REVISION, current_command)
            .map_err(|error| error.to_string())?
        {
            command::TrustDecision::Allowed(_) => self.run_current_command(),
            command::TrustDecision::ConfirmationPending => {
                self.show_command_notice("Review the command confirmation");
                Ok(())
            }
            command::TrustDecision::ConfirmationRequired(command) => {
                let dialog = adw::AlertDialog::new(
                    Some("Allow Presentation Commands?"),
                    Some(
                        "This deck can run shell commands inside Pinpoint's sandbox. Confirming trusts commands only until this presentation changes or closes.",
                    ),
                );
                let command_label = gtk::Label::builder()
                    .label(&command)
                    .selectable(true)
                    .wrap(true)
                    .xalign(0.0)
                    .build();
                command_label.add_css_class("monospace");
                command_label.set_accessible_role(gtk::AccessibleRole::Label);
                command_label
                    .update_property(&[gtk::accessible::Property::Label("Exact slide command")]);
                dialog.set_extra_child(Some(&command_label));
                dialog.add_response("cancel", "Cancel");
                dialog.add_response("run", "Run Command");
                dialog.set_response_appearance("run", adw::ResponseAppearance::Suggested);
                dialog.set_default_response(Some("cancel"));
                dialog.set_close_response("cancel");
                let weak = Rc::downgrade(&self.0);
                dialog.connect_response(None, move |_, response| {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let presenter = Self(inner);
                    if response != "run" {
                        presenter.0.command_trust.borrow_mut().dismiss();
                        return;
                    }
                    let current_command = presenter.0.presentation.slides
                        [presenter.current_slide()]
                    .command
                    .as_deref()
                    .and_then(|current| command::allow(true, Some(current)).ok());
                    if current_command != Some(command.as_str())
                        || !presenter
                            .0
                            .command_trust
                            .borrow_mut()
                            .confirm(REVISION, &command)
                    {
                        presenter.0.command_trust.borrow_mut().dismiss();
                        presenter.show_command_notice("Presentation changed — command was not run");
                        return;
                    }
                    if let Err(error) = presenter.run_current_command() {
                        presenter.show_command_notice(&error);
                    }
                });
                dialog.present(Some(window));
                Ok(())
            }
        }
    }

    fn install_source_monitor(&self) {
        if self.0.source_path.starts_with("/app/share/pinpoint") {
            return;
        }
        let file = gio::File::for_path(&self.0.source_path);
        let monitor = file
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
        let Some(monitor) = monitor else {
            eprintln!(
                "PINPOINT COMMAND source-monitor-failed path={}",
                self.0.source_path.display()
            );
            return;
        };
        monitor.set_rate_limit(100);
        let weak = Rc::downgrade(&self.0);
        monitor.connect_changed(move |_, file, other, _| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let presenter = Self(inner);
            let matches = |file: &gio::File| {
                file.path().as_deref() == Some(presenter.0.source_path.as_path())
            };
            if matches(file) || other.is_some_and(matches) {
                let revoked = presenter.0.command_trust.borrow_mut().revoke();
                if presenter.0.command_runner.is_running() {
                    presenter.0.command_runner.cancel("presentation-changed");
                    presenter.show_command_notice("Presentation changed — running command stopped");
                } else if revoked {
                    presenter
                        .show_command_notice("Presentation changed — command permission revoked");
                }
            }
        });
        *self.0.source_monitor.borrow_mut() = Some(monitor);
    }

    fn show_command_notice(&self, message: &str) {
        if let Some(source) = self.0.command_notice_control.borrow_mut().take() {
            source.remove();
        }
        self.0.command_notice_label.set_label(message);
        self.0.command_notice.set_reveal_child(true);
        let weak = Rc::downgrade(&self.0);
        let source = glib::timeout_add_local_once(Duration::from_secs(2), move || {
            if let Some(inner) = weak.upgrade() {
                inner.command_notice_control.borrow_mut().take();
                inner.command_notice.set_reveal_child(false);
            }
        });
        *self.0.command_notice_control.borrow_mut() = Some(source);
    }

    pub fn command_status(&self) -> Option<Result<(), String>> {
        self.0.command_runner.status()
    }

    pub fn mpris_bus_name(&self) -> Option<String> {
        self.0
            .mpris
            .borrow()
            .as_ref()
            .map(|mpris| mpris.bus_name().to_owned())
    }

    fn sync_mpris(&self) {
        let mpris_slot = self.0.mpris.borrow();
        let Some(mpris) = mpris_slot.as_ref() else {
            return;
        };
        mpris.sync(mpris::State {
            presenting: true,
            slide: self.current_slide(),
            slides: self.0.presentation.slides.len(),
            fullscreen: self.0.fullscreen.get(),
        });
    }

    fn maybe_autoadvance(&self) {
        if !self.0.autoadvance.get() || self.0.timer_started.borrow().is_none() {
            return;
        }
        let slide = &self.0.presentation.slides[self.current_slide()];
        if slide.duration > 0.0
            && self
                .elapsed()
                .saturating_sub(self.0.slide_started.get())
                .as_secs_f64()
                >= slide.duration
            && !self.next()
        {
            self.0.autoadvance.set(false);
        }
    }

    pub fn start_rehearsal(&self) -> Result<(), String> {
        let source_available = self.0.source_snapshot.borrow().is_some();
        if !source_available {
            return Err("the presentation source is unavailable for rehearsal writeback".into());
        }
        self.0.audience.first();
        self.0.timer_elapsed.set(Duration::ZERO);
        *self.0.timer_started.borrow_mut() = Some(Instant::now());
        *self.0.rehearsal.borrow_mut() = Some(ActiveRehearsal {
            timings: Rehearsal::new(self.0.presentation.slides.len()),
            timed_slide: self.current_slide(),
            slide_started: Duration::ZERO,
        });
        self.0.slide_started.set(Duration::ZERO);
        *self.0.rehearsal_error.borrow_mut() = None;
        self.update_views();
        Ok(())
    }

    pub fn rehearsal_active(&self) -> bool {
        self.0.rehearsal.borrow().is_some()
    }

    pub fn rehearsal_error(&self) -> Option<String> {
        self.0.rehearsal_error.borrow().clone()
    }

    fn record_rehearsal_slide(&self, expected_slide: usize) {
        let elapsed = self.elapsed();
        let mut rehearsal = self.0.rehearsal.borrow_mut();
        let Some(active) = rehearsal.as_mut() else {
            return;
        };
        if active.timed_slide != expected_slide {
            *self.0.rehearsal_error.borrow_mut() =
                Some("rehearsal timing did not match the current presentation slide".into());
            return;
        }
        if let Err(error) = active.timings.record(
            expected_slide,
            elapsed.saturating_sub(active.slide_started).as_secs_f64(),
        ) {
            *self.0.rehearsal_error.borrow_mut() = Some(error.to_string());
        }
    }

    fn begin_rehearsal_slide(&self, slide: usize) {
        if let Some(active) = self.0.rehearsal.borrow_mut().as_mut() {
            active.timed_slide = slide;
            active.slide_started = self.elapsed();
        }
    }

    fn finish_rehearsal(&self) {
        let current_slide = self.current_slide();
        self.record_rehearsal_slide(current_slide);
        let Some(active) = self.0.rehearsal.borrow_mut().take() else {
            return;
        };
        let Some(snapshot) = self.0.source_snapshot.borrow().clone() else {
            *self.0.rehearsal_error.borrow_mut() =
                Some("the presentation source is unavailable for rehearsal writeback".into());
            return;
        };
        if let Some(apply_timings) = self.0.apply_timings.as_ref() {
            apply_timings(active.timings.durations().to_vec());
            eprintln!("PINPOINT REHEARSAL applied timings to editor buffer");
            return;
        }
        match finish_to_path(&self.0.source_path, &snapshot, &active.timings) {
            Ok(updated) => {
                *self.0.source_snapshot.borrow_mut() = Some(updated);
                eprintln!("PINPOINT REHEARSAL saved timings");
            }
            Err(error) => {
                let error = error.to_string();
                eprintln!("PINPOINT REHEARSAL failed: {error}");
                *self.0.rehearsal_error.borrow_mut() = Some(error);
            }
        }
    }

    pub fn cancel_rehearsal(&self) {
        self.0.rehearsal.borrow_mut().take();
    }

    fn apply_monitor_policy(&self) {
        let monitors = model_monitors(&self.0.monitor_model);
        let previous = self.0.monitors.borrow().clone();
        let window_display = self.0.audience_window.surface().and_then(|surface| {
            gtk::prelude::WidgetExt::display(&self.0.audience_window).monitor_at_surface(&surface)
        });
        let candidates: Vec<DisplayCandidate> = monitors
            .iter()
            .enumerate()
            .map(|(index, monitor)| DisplayCandidate {
                id: index as u32,
                builtin: monitor_is_builtin(monitor),
            })
            .collect();
        let selected = choose_displays(
            &candidates,
            DisplaySelection {
                presenter: monitor_index(&monitors, previous.presenter.as_ref()),
                audience: monitor_index(&monitors, previous.audience.as_ref()),
            },
            monitor_index(&monitors, window_display.as_ref()),
        );
        let selected = RuntimeSelection {
            presenter: selected
                .presenter
                .and_then(|index| monitors.get(index as usize).cloned()),
            audience: selected
                .audience
                .and_then(|index| monitors.get(index as usize).cloned()),
        };
        *self.0.monitors.borrow_mut() = selected.clone();
        if self.0.fullscreen.get() {
            if let Some(monitor) = selected.audience.as_ref() {
                self.0.audience_window.fullscreen_on_monitor(monitor);
            } else {
                self.0.audience_window.fullscreen();
            }
            if let Some(monitor) = selected.presenter.as_ref() {
                self.0.speaker_window.fullscreen_on_monitor(monitor);
            } else {
                self.0.speaker_window.unfullscreen();
            }
        }
        eprintln!(
            "PINPOINT DISPLAYS monitors={} audience={} presenter={} fullscreen={}",
            monitors.len(),
            selected
                .audience
                .as_ref()
                .map_or_else(|| "fallback".to_owned(), monitor_name),
            selected
                .presenter
                .as_ref()
                .map_or_else(|| "fallback".to_owned(), monitor_name),
            self.0.fullscreen.get()
        );
    }

    pub fn swap_displays(&self) -> bool {
        let selected = {
            let mut selected = self.0.monitors.borrow_mut();
            if selected.presenter.is_none() || selected.audience.is_none() {
                return false;
            }
            let old_presenter = selected.presenter.take();
            selected.presenter = selected.audience.take();
            selected.audience = old_presenter;
            selected.clone()
        };
        if self.0.fullscreen.get() {
            self.reenter_fullscreen_on_selected_monitors();
        }
        eprintln!(
            "PINPOINT DISPLAYS swap audience={} presenter={} fullscreen={}",
            selected
                .audience
                .as_ref()
                .map_or_else(|| "fallback".to_owned(), monitor_name),
            selected
                .presenter
                .as_ref()
                .map_or_else(|| "fallback".to_owned(), monitor_name),
            self.0.fullscreen.get()
        );
        true
    }

    fn reenter_fullscreen_on_selected_monitors(&self) {
        // Mutter does not move a surface whose fullscreen request is already
        // active when GTK changes only its target monitor.  Complete the
        // unfullscreen transition first, then issue new monitor-specific
        // fullscreen requests from the next main-loop turn.
        self.0.audience_window.unfullscreen();
        self.0.speaker_window.unfullscreen();
        let weak = Rc::downgrade(&self.0);
        glib::timeout_add_local_once(Duration::from_millis(100), move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let presenter = Self(inner);
            if presenter.0.fullscreen.get() {
                presenter.apply_selected_monitors();
            }
        });
    }

    fn apply_selected_monitors(&self) {
        let selected = self.0.monitors.borrow();
        if let Some(monitor) = selected.audience.as_ref() {
            self.0.audience_window.fullscreen_on_monitor(monitor);
        }
        if let Some(monitor) = selected.presenter.as_ref() {
            self.0.speaker_window.fullscreen_on_monitor(monitor);
        }
    }

    pub fn toggle_fullscreen(&self) {
        let fullscreen = !self.0.fullscreen.get();
        self.0.fullscreen.set(fullscreen);
        if fullscreen {
            self.apply_monitor_policy();
        } else {
            self.0.audience_window.unfullscreen();
            self.0.speaker_window.unfullscreen();
        }
        self.sync_mpris();
    }

    pub fn stop(&self) {
        self.cancel_rehearsal();
        self.0.command_runner.cancel("presentation-closed");
        self.0.command_trust.borrow_mut().revoke();
        if let Some(monitor) = self.0.source_monitor.borrow_mut().take() {
            monitor.cancel();
        }
        if let Some(source) = self.0.timer_source.borrow_mut().take() {
            source.remove();
        }
        self.0.mpris.borrow_mut().take();
        self.0.assets.clear();
    }
}
