use crate::stage::Stage;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use pinpoint_core::{presentation, source};
use sourceview::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Duration;

const REPARSE_DELAY: Duration = Duration::from_millis(200);
const OUTLINE_SCROLL_MARGIN: f64 = 12.0;

#[derive(Clone)]
pub struct Editor(Rc<Inner>);

struct Inner {
    app: adw::Application,
    window: adw::ApplicationWindow,
    buffer: sourceview::Buffer,
    source_view: sourceview::View,
    outline: gtk::ListBox,
    outline_scroll: gtk::ScrolledWindow,
    preview: Stage,
    status: gtk::Label,
    save: gtk::Button,
    warning_tag: gtk::TextTag,
    error_tag: gtk::TextTag,
    path: RefCell<PathBuf>,
    untitled: Cell<bool>,
    setup_window: Option<adw::ApplicationWindow>,
    ignore_comments: bool,
    saved_source: RefCell<String>,
    etag: RefCell<Option<String>>,
    analysis: RefCell<source::Analysis>,
    reparse: RefCell<Option<glib::SourceId>>,
    syncing: Cell<bool>,
    monitor: RefCell<Option<gio::FileMonitor>>,
    active_speaker: RefCell<Option<crate::speaker::SpeakerPresenter>>,
    keepalive: RefCell<Option<Editor>>,
}

fn buffer_text(buffer: &sourceview::Buffer) -> String {
    let (start, end) = buffer.bounds();
    buffer.text(&start, &end, true).to_string()
}

fn file_etag(file: &gio::File) -> Result<Option<String>, String> {
    file.query_info(
        "etag::value",
        gio::FileQueryInfoFlags::NONE,
        None::<&gio::Cancellable>,
    )
    .map(|info| {
        info.attribute_string("etag::value")
            .map(|etag| etag.to_string())
    })
    .map_err(|error| error.to_string())
}

fn load_source(path: &Path) -> Result<(String, Option<String>), String> {
    let file = gio::File::for_path(path);
    let before = file_etag(&file)?;
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > presentation::MAX_PRESENTATION_BYTES {
        return Err(format!(
            "presentation exceeds the {} MiB safety limit",
            presentation::MAX_PRESENTATION_BYTES / 1024 / 1024
        ));
    }
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let after = file_etag(&file)?;
    if before != after {
        return Err("presentation changed while it was being read; try again".into());
    }
    Ok((source, after))
}

fn byte_to_char_offset(source: &str, offset: usize) -> i32 {
    source[..offset.min(source.len())].chars().count() as i32
}

fn source_offset(buffer: &sourceview::Buffer, iter: &gtk::TextIter) -> usize {
    let start = buffer.start_iter();
    buffer.text(&start, iter, true).len()
}

fn editor_title(path: &Path) -> String {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Presentation");
    format!("Compose Presentation — {filename}")
}

fn available_asset_path(directory: &Path, filename: &Path) -> PathBuf {
    let requested = directory.join(filename);
    if !requested.exists() {
        return requested;
    }
    let stem = filename
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("asset");
    let extension = filename
        .extension()
        .and_then(|extension| extension.to_str());
    for suffix in 2.. {
        let candidate = match extension {
            Some(extension) => directory.join(format!("{stem}-{suffix}.{extension}")),
            None => directory.join(format!("{stem}-{suffix}")),
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn outline_scroll_value(
    current: f64,
    page_size: f64,
    upper: f64,
    row_top: f64,
    row_bottom: f64,
) -> f64 {
    let visible_bottom = current + page_size;
    let target = if row_top < current {
        row_top - OUTLINE_SCROLL_MARGIN
    } else if row_bottom > visible_bottom {
        row_bottom + OUTLINE_SCROLL_MARGIN - page_size
    } else {
        current
    };
    target.clamp(0.0, (upper - page_size).max(0.0))
}

impl Editor {
    fn path(&self) -> PathBuf {
        self.0.path.borrow().clone()
    }

    pub fn open(
        app: &adw::Application,
        path: PathBuf,
        ignore_comments: bool,
        setup_window: Option<adw::ApplicationWindow>,
    ) -> Result<Self, String> {
        let (initial_source, etag) = load_source(&path)?;
        Self::open_with_source(
            app,
            path,
            initial_source,
            false,
            etag,
            ignore_comments,
            setup_window,
        )
    }

    pub fn open_untitled(
        app: &adw::Application,
        setup_window: Option<adw::ApplicationWindow>,
    ) -> Result<Self, String> {
        Self::open_with_source(
            app,
            PathBuf::from("Untitled.pin"),
            "[font=Sans 48px]\n\n--\n# New Presentation\n".into(),
            true,
            None,
            false,
            setup_window,
        )
    }

    fn open_with_source(
        app: &adw::Application,
        path: PathBuf,
        initial_source: String,
        untitled: bool,
        etag: Option<String>,
        ignore_comments: bool,
        setup_window: Option<adw::ApplicationWindow>,
    ) -> Result<Self, String> {
        let languages = sourceview::LanguageManager::default();
        for directory in [
            PathBuf::from("/app/share/gtksourceview-5/language-specs"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
        ] {
            if directory.is_dir() {
                languages.append_search_path(&directory.to_string_lossy());
            }
        }
        let language = languages.language("pinpoint");
        let buffer = sourceview::Buffer::new(None);
        buffer.set_language(language.as_ref());
        buffer.set_highlight_syntax(true);
        let schemes = sourceview::StyleSchemeManager::default();
        for directory in [
            PathBuf::from("/app/share/gtksourceview-5/styles"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
        ] {
            if directory.is_dir() {
                schemes.append_search_path(&directory.to_string_lossy());
            }
        }
        let style_manager = adw::StyleManager::default();
        let scheme = if style_manager.is_dark() {
            schemes.scheme("Pinpoint-dark")
        } else {
            schemes.scheme("Pinpoint")
        };
        buffer.set_style_scheme(scheme.as_ref());
        let buffer_for_style = buffer.clone();
        style_manager.connect_dark_notify(move |manager| {
            let scheme = if manager.is_dark() {
                schemes.scheme("Pinpoint-dark")
            } else {
                schemes.scheme("Pinpoint")
            };
            buffer_for_style.set_style_scheme(scheme.as_ref());
        });
        let warning_tag = gtk::TextTag::builder()
            .name("pinpoint-editor-warning")
            .underline(gtk::pango::Underline::Error)
            .underline_rgba(&gdk::RGBA::new(0.95, 0.65, 0.1, 1.0))
            .build();
        let error_tag = gtk::TextTag::builder()
            .name("pinpoint-editor-error")
            .underline(gtk::pango::Underline::Error)
            .underline_rgba(&gdk::RGBA::new(0.95, 0.25, 0.2, 1.0))
            .build();
        let table = buffer.tag_table();
        table.add(&warning_tag);
        table.add(&error_tag);
        buffer.set_text(&initial_source);
        buffer.set_modified(false);
        let source_view = sourceview::View::with_buffer(&buffer);
        source_view.set_show_line_numbers(true);
        source_view.set_highlight_current_line(true);
        source_view.set_auto_indent(true);
        source_view.set_monospace(true);
        source_view.set_wrap_mode(gtk::WrapMode::WordChar);

        let preview = Stage::default();
        preview.set_audio_enabled(false);
        preview.set_camera_enabled(false);
        preview.set_accessible_context("Editor preview");
        let outline = gtk::ListBox::new();
        outline.set_selection_mode(gtk::SelectionMode::Single);
        outline.add_css_class("navigation-sidebar");
        outline.set_size_request(220, -1);
        let outline_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&outline)
            .build();
        outline_scroll.add_css_class("sidebar");
        let status = gtk::Label::new(Some("Preparing preview…"));
        status.add_css_class("dim-label");
        status.set_xalign(0.0);
        let save = gtk::Button::with_label("Save");
        save.set_sensitive(false);
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(editor_title(&path))
            .default_width(1200)
            .default_height(760)
            .build();
        let editor = Self(Rc::new(Inner {
            app: app.clone(),
            window,
            buffer,
            source_view,
            outline,
            outline_scroll,
            preview,
            status,
            save,
            warning_tag,
            error_tag,
            path: RefCell::new(path),
            untitled: Cell::new(untitled),
            setup_window,
            ignore_comments,
            saved_source: RefCell::new(initial_source),
            etag: RefCell::new(etag),
            analysis: RefCell::new(source::Analysis::default()),
            reparse: RefCell::new(None),
            syncing: Cell::new(false),
            monitor: RefCell::new(None),
            active_speaker: RefCell::new(None),
            keepalive: RefCell::new(None),
        }));
        *editor.0.keepalive.borrow_mut() = Some(editor.clone());
        editor.build_ui();
        editor.install_handlers();
        editor.reparse_now();
        editor.monitor_file();
        editor.0.window.present();
        Ok(editor)
    }

    fn build_ui(&self) {
        let toolbar = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        crate::app_shell::add_application_menu(&self.0.window, &header);
        header.set_title_widget(Some(&adw::WindowTitle::new("Compose Presentation", "")));
        let back = gtk::Button::from_icon_name("go-previous-symbolic");
        back.set_tooltip_text(Some("Back to presentation setup"));
        back.update_property(&[gtk::accessible::Property::Label(
            "Back to presentation setup",
        )]);
        let present = gtk::Button::with_label("Present");
        present.add_css_class("suggested-action");
        present.update_property(&[gtk::accessible::Property::KeyShortcuts("Ctrl+Enter")]);
        let rehearse = gtk::Button::with_label("Rehearse");
        rehearse.update_property(&[gtk::accessible::Property::KeyShortcuts("Ctrl+Shift+R")]);
        let save_as = gtk::Button::with_label("Save As…");
        save_as.update_property(&[gtk::accessible::Property::KeyShortcuts("Ctrl+Shift+S")]);
        let import_asset = gtk::Button::from_icon_name("insert-image-symbolic");
        import_asset.set_tooltip_text(Some("Import Asset…"));
        import_asset.update_property(&[
            gtk::accessible::Property::Label("Import Asset"),
            gtk::accessible::Property::KeyShortcuts("Ctrl+I"),
        ]);
        self.0
            .save
            .update_property(&[gtk::accessible::Property::KeyShortcuts("Ctrl+S")]);
        header.pack_start(&back);
        header.pack_start(&import_asset);
        header.pack_end(&self.0.save);
        header.pack_end(&save_as);
        header.pack_end(&rehearse);
        header.pack_end(&present);
        toolbar.add_top_bar(&header);

        let source_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&self.0.source_view)
            .build();
        let preview_frame = gtk::AspectFrame::new(0.5, 0.5, 16.0 / 9.0, false);
        preview_frame.set_child(Some(&self.0.preview));
        preview_frame.set_hexpand(true);
        preview_frame.set_vexpand(true);
        preview_frame.add_css_class("card");
        preview_frame.set_margin_start(12);
        preview_frame.set_margin_end(12);
        preview_frame.set_margin_top(12);
        preview_frame.set_margin_bottom(12);
        let work = gtk::Paned::new(gtk::Orientation::Horizontal);
        work.set_start_child(Some(&source_scroll));
        work.set_end_child(Some(&preview_frame));
        work.set_position(620);
        let outer = gtk::Paned::new(gtk::Orientation::Horizontal);
        outer.set_start_child(Some(&self.0.outline_scroll));
        outer.set_end_child(Some(&work));
        outer.set_resize_start_child(false);
        outer.set_position(230);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        self.0.status.set_margin_start(8);
        self.0.status.set_margin_end(8);
        self.0.status.set_margin_top(4);
        self.0.status.set_margin_bottom(4);
        content.append(&self.0.status);
        content.append(&outer);
        toolbar.set_content(Some(&content));
        self.0.window.set_content(Some(&toolbar));

        let weak = Rc::downgrade(&self.0);
        self.0.save.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                Self(inner).save();
            }
        });
        let weak = Rc::downgrade(&self.0);
        save_as.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                Self(inner).save_as();
            }
        });
        let weak = Rc::downgrade(&self.0);
        import_asset.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                Self(inner).import_asset();
            }
        });
        let weak = Rc::downgrade(&self.0);
        back.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                Self(inner).request_return_to_setup();
            }
        });
        for (button, rehearse) in [(&present, false), (&rehearse, true)] {
            let weak = Rc::downgrade(&self.0);
            button.connect_clicked(move |_| {
                if let Some(inner) = weak.upgrade() {
                    let editor = Self(inner);
                    if rehearse {
                        editor.launch_rehearsal();
                    } else {
                        let _ = editor.launch_presenter();
                    }
                }
            });
        }
    }

    fn install_handlers(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0.buffer.connect_changed(move |_| {
            if let Some(inner) = weak.upgrade() {
                let editor = Self(inner);
                editor.0.save.set_sensitive(true);
                editor.schedule_reparse();
            }
        });
        let weak = Rc::downgrade(&self.0);
        self.0.buffer.connect_cursor_position_notify(move |buffer| {
            if let Some(inner) = weak.upgrade() {
                let editor = Self(inner);
                if editor.0.syncing.get() {
                    return;
                }
                let iter = buffer.iter_at_offset(buffer.cursor_position());
                let offset = source_offset(buffer, &iter);
                let index = source::find_slide(&editor.0.analysis.borrow(), offset);
                editor.select_slide(index, false);
            }
        });
        let weak = Rc::downgrade(&self.0);
        self.0.outline.connect_row_selected(move |_, row| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let editor = Self(inner);
            if editor.0.syncing.get() {
                return;
            }
            if let Some(row) = row {
                editor.select_slide(row.index().max(0) as usize, true);
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&self.0);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(inner) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let editor = Self(inner);
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK) && key == gdk::Key::s {
                if modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
                    editor.save_as();
                } else {
                    editor.save();
                }
                return glib::Propagation::Stop;
            }
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK) && key == gdk::Key::space {
                editor.show_completion();
                return glib::Propagation::Stop;
            }
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK) && key == gdk::Key::i {
                editor.import_asset();
                return glib::Propagation::Stop;
            }
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK) && key == gdk::Key::Return {
                let _ = editor.launch_presenter();
                return glib::Propagation::Stop;
            }
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK)
                && modifiers.contains(gdk::ModifierType::SHIFT_MASK)
                && key == gdk::Key::r
            {
                editor.launch_rehearsal();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.0.source_view.add_controller(keys);
        let weak = Rc::downgrade(&self.0);
        self.0.window.connect_close_request(move |_| {
            let Some(inner) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let editor = Self(inner);
            if editor.0.buffer.is_modified() {
                editor.show_unsaved_prompt(false);
                glib::Propagation::Stop
            } else {
                editor.close_active_speaker();
                editor.close_hidden_setup_window();
                editor.release_keepalive();
                glib::Propagation::Proceed
            }
        });
        let weak = Rc::downgrade(&self.0);
        self.0.window.connect_destroy(move |_| {
            if let Some(inner) = weak.upgrade() {
                let editor = Self(inner);
                editor.close_active_speaker();
                editor.release_keepalive();
            }
        });
    }

    fn schedule_reparse(&self) {
        if let Some(source) = self.0.reparse.borrow_mut().take() {
            source.remove();
        }
        let weak = Rc::downgrade(&self.0);
        let source = glib::timeout_add_local_once(REPARSE_DELAY, move || {
            if let Some(inner) = weak.upgrade() {
                let editor = Self(inner);
                editor.0.reparse.borrow_mut().take();
                editor.reparse_now();
            }
        });
        *self.0.reparse.borrow_mut() = Some(source);
    }

    fn reparse_now(&self) {
        let source_text = buffer_text(&self.0.buffer);
        let analysis = source::analyze(&source_text);
        let cursor = self
            .0
            .buffer
            .iter_at_offset(self.0.buffer.cursor_position());
        let selected = source::find_slide(&analysis, source_offset(&self.0.buffer, &cursor));
        self.rebuild_outline(&analysis, selected);
        self.apply_diagnostics(&analysis, &source_text);
        *self.0.analysis.borrow_mut() = analysis.clone();
        let path = self.path();
        match presentation::parse(&source_text, Some(&path), self.0.ignore_comments) {
            Ok(presentation) => {
                let slides = presentation.slides.len();
                self.0
                    .preview
                    .set_presentation_at(presentation, path, selected);
                self.0.status.set_text(&format!(
                    "{slides} slide{} · {} problem{} · safe preview",
                    if slides == 1 { "" } else { "s" },
                    analysis.diagnostics.len(),
                    if analysis.diagnostics.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                ));
            }
            Err(error) => self.0.status.set_text(&format!("Preview paused · {error}")),
        }
    }

    fn rebuild_outline(&self, analysis: &source::Analysis, selected: usize) {
        while let Some(child) = self.0.outline.first_child() {
            self.0.outline.remove(&child);
        }
        for (index, slide) in analysis.slides.iter().enumerate() {
            let row = gtk::ListBoxRow::new();
            let text = gtk::Label::new(Some(&format!("{:>3}  {}", index + 1, slide.title)));
            text.set_xalign(0.0);
            text.set_wrap(true);
            text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            text.set_margin_start(8);
            text.set_margin_end(8);
            text.set_margin_top(5);
            text.set_margin_bottom(5);
            row.set_child(Some(&text));
            self.0.outline.append(&row);
        }
        self.0.syncing.set(true);
        if let Some(row) = self.0.outline.row_at_index(selected as i32) {
            self.0.outline.select_row(Some(&row));
        }
        self.0.syncing.set(false);
    }

    fn apply_diagnostics(&self, analysis: &source::Analysis, source_text: &str) {
        let (start, end) = self.0.buffer.bounds();
        self.0.buffer.remove_tag(&self.0.warning_tag, &start, &end);
        self.0.buffer.remove_tag(&self.0.error_tag, &start, &end);
        for diagnostic in &analysis.diagnostics {
            let start = self
                .0
                .buffer
                .iter_at_offset(byte_to_char_offset(source_text, diagnostic.start));
            let end = self
                .0
                .buffer
                .iter_at_offset(byte_to_char_offset(source_text, diagnostic.end));
            let tag = match diagnostic.severity {
                source::DiagnosticSeverity::Warning => &self.0.warning_tag,
                source::DiagnosticSeverity::Error => &self.0.error_tag,
            };
            self.0.buffer.apply_tag(tag, &start, &end);
        }
        self.0.status.set_tooltip_text(
            analysis
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.as_str()),
        );
    }

    fn show_completion(&self) {
        let list = gtk::ListBox::new();
        let source_text = buffer_text(&self.0.buffer);
        let iter = self
            .0
            .buffer
            .iter_at_offset(self.0.buffer.cursor_position());
        let completions = Rc::new(source::complete(
            &source_text,
            Some(&self.path()),
            source_offset(&self.0.buffer, &iter),
        ));
        if completions.is_empty() {
            return;
        }
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(true);
        let popover = gtk::Popover::new();
        for completion in completions.iter() {
            let row = gtk::ListBoxRow::new();
            let label = gtk::Label::new(Some(&format!(
                "{}\n{}",
                completion.label, completion.detail
            )));
            label.set_xalign(0.0);
            label.set_margin_start(8);
            label.set_margin_end(8);
            label.set_margin_top(4);
            label.set_margin_bottom(4);
            row.set_child(Some(&label));
            list.append(&row);
        }
        let weak = Rc::downgrade(&self.0);
        let popover_for_rows = popover.clone();
        let completions_for_rows = Rc::clone(&completions);
        list.connect_row_activated(move |_, row| {
            if let (Some(inner), Some(completion)) = (
                weak.upgrade(),
                completions_for_rows.get(row.index().max(0) as usize),
            ) {
                Self(inner).apply_completion(completion);
            }
            popover_for_rows.popdown();
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&self.0);
        let list_for_keys = list.clone();
        let popover_for_keys = popover.clone();
        let completions_for_keys = Rc::clone(&completions);
        keys.connect_key_pressed(move |_, key, _, _| match key {
            gdk::Key::Escape => {
                popover_for_keys.popdown();
                if let Some(inner) = weak.upgrade() {
                    inner.source_view.grab_focus();
                }
                glib::Propagation::Stop
            }
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::Tab => {
                if let (Some(inner), Some(row)) = (weak.upgrade(), list_for_keys.selected_row())
                    && let Some(completion) = completions_for_keys.get(row.index().max(0) as usize)
                {
                    Self(inner).apply_completion(completion);
                }
                popover_for_keys.popdown();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        list.add_controller(keys);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(240)
            .max_content_height(420)
            .child(&list)
            .build();
        popover.set_child(Some(&scroll));
        popover.set_position(gtk::PositionType::Bottom);
        popover.set_parent(&self.0.source_view);
        popover.popup();
        if let Some(row) = list.row_at_index(0) {
            list.select_row(Some(&row));
        }
        list.grab_focus();
    }

    fn apply_completion(&self, completion: &source::Completion) {
        let source_text = buffer_text(&self.0.buffer);
        let mut start = self
            .0
            .buffer
            .iter_at_offset(byte_to_char_offset(&source_text, completion.replace_start));
        let mut cursor = self
            .0
            .buffer
            .iter_at_offset(self.0.buffer.cursor_position());
        self.0.buffer.delete(&mut start, &mut cursor);
        self.0.buffer.insert(&mut start, &completion.insert_text);
        for _ in 0..completion.cursor_back {
            start.backward_char();
        }
        self.0.buffer.place_cursor(&start);
        self.0.source_view.grab_focus();
    }

    fn select_slide(&self, index: usize, move_cursor: bool) {
        let analysis = self.0.analysis.borrow();
        let Some(slide) = analysis.slides.get(index) else {
            return;
        };
        self.0.preview.set_slide_without_transition(index);
        self.0.syncing.set(true);
        if let Some(row) = self.0.outline.row_at_index(index as i32) {
            self.0.outline.select_row(Some(&row));
            self.reveal_outline_row(&row);
        }
        if move_cursor {
            let source_text = buffer_text(&self.0.buffer);
            let offset = byte_to_char_offset(&source_text, slide.separator_end.saturating_add(1));
            let mut iter = self.0.buffer.iter_at_offset(offset);
            self.0.buffer.place_cursor(&iter);
            self.0
                .source_view
                .scroll_to_iter(&mut iter, 0.15, false, 0.0, 0.0);
        }
        self.0.syncing.set(false);
    }

    fn reveal_outline_row(&self, row: &gtk::ListBoxRow) {
        let scroll = self.0.outline_scroll.clone();
        let outline = self.0.outline.clone();
        let row = row.clone();
        glib::idle_add_local_once(move || {
            let Some(bounds) = row.compute_bounds(&outline) else {
                return;
            };
            let adjustment = scroll.vadjustment();
            let value = outline_scroll_value(
                adjustment.value(),
                adjustment.page_size(),
                adjustment.upper(),
                bounds.y() as f64,
                (bounds.y() + bounds.height()) as f64,
            );
            if (adjustment.value() - value).abs() > f64::EPSILON {
                adjustment.set_value(value);
            }
        });
    }

    fn save(&self) {
        if self.0.untitled.get() {
            self.save_as();
            return;
        }
        self.finish_save(gio::File::for_path(self.path()), None);
    }

    fn save_as(&self) {
        self.save_as_then(None);
    }

    fn save_as_then(&self, after_save: Option<Rc<dyn Fn(Self)>>) {
        let dialog = gtk::FileDialog::new();
        dialog.set_title("Save Presentation As");
        let suggested = self
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("presentation.pin")
            .to_owned();
        dialog.set_initial_name(Some(&suggested));
        let weak = Rc::downgrade(&self.0);
        dialog.save(
            Some(&self.0.window),
            None::<&gio::Cancellable>,
            move |result| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let editor = Self(inner);
                let Ok(file) = result else {
                    return;
                };
                let Some(path) = file.path() else {
                    editor
                        .0
                        .status
                        .set_text("Save As needs a local presentation file");
                    return;
                };
                if editor.finish_save(file, Some(path))
                    && let Some(after_save) = &after_save
                {
                    after_save(editor);
                }
            },
        );
    }

    fn import_asset(&self) {
        if self.0.untitled.get() {
            self.0
                .status
                .set_text("Save the presentation before importing an asset");
            let after_save: Rc<dyn Fn(Self)> = Rc::new(|editor| editor.choose_asset());
            self.save_as_then(Some(after_save));
            return;
        }
        self.choose_asset();
    }

    fn choose_asset(&self) {
        let dialog = gtk::FileDialog::builder()
            .title("Import Asset")
            .accept_label("Import")
            .build();
        let weak = Rc::downgrade(&self.0);
        dialog.open(
            Some(&self.0.window),
            None::<&gio::Cancellable>,
            move |result| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let editor = Self(inner);
                let Ok(source_file) = result else {
                    return;
                };
                editor.copy_imported_asset(source_file);
            },
        );
    }

    fn copy_imported_asset(&self, source_file: gio::File) {
        let Some(filename) = source_file.basename() else {
            self.0.status.set_text("Unable to import an unnamed asset");
            return;
        };
        let Some(filename_text) = filename.to_str() else {
            self.0
                .status
                .set_text("The asset filename is not valid UTF-8");
            return;
        };
        if filename_text.contains(['[', ']', '\n', '\r']) {
            self.0.status.set_text(
                "The asset filename contains characters unsupported by presentation settings",
            );
            return;
        }
        let Some(directory) = self.path().parent().map(Path::to_path_buf) else {
            self.0
                .status
                .set_text("The presentation has no directory for relative assets");
            return;
        };
        let requested = directory.join(&filename);
        let requested_file = gio::File::for_path(&requested);
        if source_file.equal(&requested_file) {
            self.insert_imported_asset(filename_text);
            return;
        }
        let destination = available_asset_path(&directory, &filename);
        let imported_name = destination
            .file_name()
            .and_then(|name| name.to_str())
            .expect("derived asset filenames remain valid UTF-8")
            .to_owned();
        let destination_file = gio::File::for_path(&destination);
        self.0
            .status
            .set_text(&format!("Importing {imported_name}…"));
        let weak = Rc::downgrade(&self.0);
        source_file.copy_async(
            &destination_file,
            gio::FileCopyFlags::NONE,
            glib::Priority::DEFAULT,
            None::<&gio::Cancellable>,
            None,
            move |result| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let editor = Self(inner);
                match result {
                    Ok(()) => editor.insert_imported_asset(&imported_name),
                    Err(error) => editor
                        .0
                        .status
                        .set_text(&format!("Unable to import asset · {error}")),
                }
            },
        );
    }

    fn insert_imported_asset(&self, filename: &str) {
        let source_text = buffer_text(&self.0.buffer);
        let cursor = self
            .0
            .buffer
            .iter_at_offset(self.0.buffer.cursor_position());
        let Some(edit) = source::imported_asset_edit(
            &source_text,
            source_offset(&self.0.buffer, &cursor),
            filename,
        ) else {
            self.0
                .status
                .set_text("Unable to insert the imported asset into this presentation");
            return;
        };
        let mut start = self
            .0
            .buffer
            .iter_at_offset(byte_to_char_offset(&source_text, edit.replace_start));
        let mut end = self
            .0
            .buffer
            .iter_at_offset(byte_to_char_offset(&source_text, edit.replace_end));
        self.0.buffer.begin_user_action();
        self.0.buffer.delete(&mut start, &mut end);
        self.0.buffer.insert(&mut start, &edit.insert_text);
        self.0.buffer.end_user_action();
        self.0.buffer.place_cursor(&start);
        self.0.source_view.grab_focus();
        self.0
            .status
            .set_text(&format!("Imported {filename} · save when ready"));
    }

    fn finish_save(&self, file: gio::File, replacement_path: Option<PathBuf>) -> bool {
        let source_text = buffer_text(&self.0.buffer);
        let expected_etag = replacement_path
            .is_none()
            .then(|| self.0.etag.borrow().clone())
            .flatten();
        match file.replace_contents(
            source_text.as_bytes(),
            expected_etag.as_deref(),
            false,
            gio::FileCreateFlags::REPLACE_DESTINATION,
            None::<&gio::Cancellable>,
        ) {
            Ok(new_etag) => {
                if let Some(path) = replacement_path {
                    *self.0.path.borrow_mut() = path.clone();
                    self.0.untitled.set(false);
                    self.0.window.set_title(Some(&editor_title(&path)));
                    self.monitor_file();
                }
                *self.0.saved_source.borrow_mut() = source_text;
                *self.0.etag.borrow_mut() = new_etag.map(|etag| etag.to_string());
                self.0.buffer.set_modified(false);
                self.0.save.set_sensitive(false);
                self.0.status.set_text("Saved · live preview remains local");
                true
            }
            Err(error) => {
                self.0.status.set_text(&format!("Unable to save · {error}"));
                false
            }
        }
    }

    fn launch_presenter(&self) -> Option<adw::ApplicationWindow> {
        let source_text = buffer_text(&self.0.buffer);
        let presentation =
            match presentation::parse(&source_text, Some(&self.path()), self.0.ignore_comments) {
                Ok(presentation) => presentation,
                Err(error) => {
                    self.0.status.set_text(&format!("Preview paused · {error}"));
                    return None;
                }
            };
        let window = crate::open_editor_snapshot_presenter(
            &self.0.app,
            presentation,
            self.path(),
            self.0.preview.current_slide(),
            &self.0.window,
        );
        self.0
            .status
            .set_text("Presenting the current editor buffer");
        Some(window)
    }

    fn launch_rehearsal(&self) {
        if let Some(presenter) = self.0.active_speaker.borrow_mut().take() {
            presenter.close();
        }
        let source_text = buffer_text(&self.0.buffer);
        let presentation =
            match presentation::parse(&source_text, Some(&self.path()), self.0.ignore_comments) {
                Ok(presentation) => presentation,
                Err(error) => {
                    self.0.status.set_text(&format!("Preview paused · {error}"));
                    return;
                }
            };
        let weak = Rc::downgrade(&self.0);
        let presenter = crate::speaker::SpeakerPresenter::new_for_editor(
            &self.0.app,
            presentation,
            self.path(),
            source_text,
            move |durations| {
                if let Some(inner) = weak.upgrade() {
                    Self(inner).apply_timings(durations);
                }
            },
        );
        if let Err(error) = presenter.start_rehearsal() {
            self.0
                .status
                .set_text(&format!("Unable to start rehearsal · {error}"));
            presenter.stop();
            return;
        }
        self.0
            .status
            .set_text("Rehearsal started · timings will update this buffer");
        *self.0.active_speaker.borrow_mut() = Some(presenter);
    }

    fn apply_timings(&self, durations: Vec<f64>) {
        let source_text = buffer_text(&self.0.buffer);
        let updated = match source::apply_durations(&source_text, &durations) {
            Ok(updated) => updated,
            Err(error) => {
                self.0
                    .status
                    .set_text(&format!("Unable to apply timings · {error}"));
                return;
            }
        };
        let (mut start, mut end) = self.0.buffer.bounds();
        self.0.buffer.begin_user_action();
        self.0.buffer.delete(&mut start, &mut end);
        self.0.buffer.insert(&mut start, &updated);
        self.0.buffer.end_user_action();
        self.0.buffer.set_modified(true);
        self.0.save.set_sensitive(true);
        self.reparse_now();
        self.0.status.set_text("Timings applied · save when ready");
    }

    fn request_return_to_setup(&self) {
        if self.0.buffer.is_modified() {
            self.show_unsaved_prompt(true);
        } else {
            self.return_to_setup();
        }
    }

    fn return_to_setup(&self) {
        self.close_active_speaker();
        if let Some(window) = &self.0.setup_window {
            window.present();
        } else {
            self.0.app.activate();
        }
        self.release_keepalive();
        self.0.window.close();
    }

    fn close_hidden_setup_window(&self) {
        if let Some(window) = &self.0.setup_window
            && !window.is_visible()
        {
            window.close();
        }
    }

    fn finish_close_request(&self, return_to_setup: bool) {
        if return_to_setup {
            self.return_to_setup();
        } else {
            self.close_active_speaker();
            self.release_keepalive();
            self.0.window.close();
        }
    }

    fn close_active_speaker(&self) {
        if let Some(presenter) = self.0.active_speaker.borrow_mut().take() {
            presenter.close();
        }
    }

    fn release_keepalive(&self) {
        self.0.keepalive.borrow_mut().take();
    }

    fn show_unsaved_prompt(&self, return_to_setup: bool) {
        let dialog = adw::AlertDialog::new(
            Some("Save Changes?"),
            Some("Unsaved changes to this presentation will be lost."),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("discard", "Discard");
        dialog.add_response("save", "Save");
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("cancel");
        let weak = Rc::downgrade(&self.0);
        dialog.connect_response(None, move |_, response| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let editor = Self(inner);
            match response {
                "save" => {
                    if editor.0.untitled.get() {
                        let after_save: Rc<dyn Fn(Self)> = Rc::new(move |editor| {
                            editor.finish_close_request(return_to_setup);
                        });
                        editor.save_as_then(Some(after_save));
                    } else {
                        editor.save();
                        if !editor.0.buffer.is_modified() {
                            editor.finish_close_request(return_to_setup);
                        }
                    }
                }
                "discard" => {
                    editor.0.buffer.set_modified(false);
                    editor.finish_close_request(return_to_setup);
                }
                _ => {}
            }
        });
        dialog.present(Some(&self.0.window));
    }

    fn monitor_file(&self) {
        if self.0.untitled.get() {
            return;
        }
        let file = gio::File::for_path(self.path());
        let Ok(monitor) = file.monitor_file(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>)
        else {
            return;
        };
        let weak: Weak<Inner> = Rc::downgrade(&self.0);
        monitor.connect_changed(move |_, _, _, event| {
            if !matches!(
                event,
                gio::FileMonitorEvent::Changed
                    | gio::FileMonitorEvent::ChangesDoneHint
                    | gio::FileMonitorEvent::Renamed
            ) {
                return;
            }
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let editor = Self(inner);
            if editor.0.buffer.is_modified() {
                editor
                    .0
                    .status
                    .set_text("External change detected · save refused");
                return;
            }
            match load_source(&editor.path()) {
                Ok((source_text, etag)) if source_text != *editor.0.saved_source.borrow() => {
                    *editor.0.saved_source.borrow_mut() = source_text.clone();
                    *editor.0.etag.borrow_mut() = etag;
                    editor.0.buffer.set_text(&source_text);
                    editor.0.buffer.set_modified(false);
                    editor.0.save.set_sensitive(false);
                    editor.reparse_now();
                    editor.0.status.set_text("Reloaded external change");
                }
                Ok((_, etag)) => *editor.0.etag.borrow_mut() = etag,
                Err(error) => editor
                    .0
                    .status
                    .set_text(&format!("External change · {error}")),
            }
        });
        *self.0.monitor.borrow_mut() = Some(monitor);
    }

    pub fn status(&self) -> String {
        self.0.status.text().to_string()
    }

    pub fn outline_count(&self) -> usize {
        self.0.analysis.borrow().slides.len()
    }

    pub fn language_loaded(&self) -> bool {
        self.0.buffer.language().is_some()
    }

    pub fn diagnostics_decorated(&self) -> bool {
        let source_text = buffer_text(&self.0.buffer);
        self.0
            .analysis
            .borrow()
            .diagnostics
            .iter()
            .all(|diagnostic| {
                let iter = self
                    .0
                    .buffer
                    .iter_at_offset(byte_to_char_offset(&source_text, diagnostic.start));
                iter.has_tag(&self.0.warning_tag) || iter.has_tag(&self.0.error_tag)
            })
    }

    pub fn apply_rehearsal_durations(&self, durations: Vec<f64>) {
        self.apply_timings(durations);
    }

    pub fn present_unsaved_snapshot(&self) -> Option<adw::ApplicationWindow> {
        self.launch_presenter()
    }

    pub fn is_visible(&self) -> bool {
        self.0.window.is_visible()
    }

    pub fn is_modified(&self) -> bool {
        self.0.buffer.is_modified()
    }
}

pub fn validate_hidden_parent_lifecycle(app: &adw::Application) -> Result<(), String> {
    let hidden_parent = adw::ApplicationWindow::builder()
        .application(app)
        .title("Hidden setup lifecycle validation")
        .build();
    hidden_parent.present();
    hidden_parent.set_visible(false);
    let closing_editor = Editor::open_untitled(app, Some(hidden_parent.clone()))?;
    closing_editor.0.window.close();
    let hidden_parent_registered = app
        .windows()
        .iter()
        .any(|window| window == hidden_parent.upcast_ref::<gtk::Window>());
    if closing_editor.is_visible() || hidden_parent.is_visible() || hidden_parent_registered {
        return Err("closing an editor retained its hidden setup window".into());
    }

    let returning_parent = adw::ApplicationWindow::builder()
        .application(app)
        .title("Return-to-setup lifecycle validation")
        .build();
    returning_parent.present();
    returning_parent.set_visible(false);
    let returning_editor = Editor::open_untitled(app, Some(returning_parent.clone()))?;
    returning_editor.return_to_setup();
    let returning_parent_registered = app
        .windows()
        .iter()
        .any(|window| window == returning_parent.upcast_ref::<gtk::Window>());
    if returning_editor.is_visible()
        || !returning_parent.is_visible()
        || !returning_parent_registered
    {
        return Err("Back did not restore and retain the setup window".into());
    }
    returning_parent.close();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_reveal_scrolls_only_when_the_selected_slide_is_outside_view() {
        assert_eq!(
            outline_scroll_value(100.0, 200.0, 1_000.0, 180.0, 220.0),
            100.0
        );
        assert_eq!(
            outline_scroll_value(100.0, 200.0, 1_000.0, 40.0, 80.0),
            28.0
        );
        assert_eq!(
            outline_scroll_value(100.0, 200.0, 1_000.0, 330.0, 370.0),
            182.0
        );
    }

    #[test]
    fn compose_title_uses_only_the_filename() {
        assert_eq!(
            editor_title(Path::new("/run/user/1000/doc/opaque/deck/talk.pin")),
            "Compose Presentation — talk.pin"
        );
    }

    #[test]
    fn imported_assets_get_non_destructive_collision_names() {
        let directory =
            std::env::temp_dir().join(format!("pinpoint-editor-import-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("photo.png"), b"one").unwrap();
        std::fs::write(directory.join("photo-2.png"), b"two").unwrap();
        assert_eq!(
            available_asset_path(&directory, Path::new("photo.png")),
            directory.join("photo-3.png")
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
