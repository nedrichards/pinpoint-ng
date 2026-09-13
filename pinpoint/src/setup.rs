use crate::app_shell;
use crate::pdf;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use pinpoint_core::presentation::{self, BackgroundType, Presentation};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const APP_ID: &str = "com.nedrichards.pinpoint";
const DOCUMENT_PORTAL_HOST_PATH_ATTRIBUTE: &str = "xattr::document-portal.host-path";
const MAX_FOLDER_ENTRIES: usize = 10_000;
const MAX_PRESENTATION_FILES: usize = 1_000;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LaunchOptions {
    pub presentation: PathBuf,
    pub presentation_label: String,
    pub fullscreen: bool,
    pub speaker_mode: bool,
    pub rehearse: bool,
    pub audience_monitor: Option<String>,
    pub ignore_comments: bool,
}

#[derive(Clone, Debug)]
struct PresentationSelection {
    path: PathBuf,
    label: String,
    details: String,
    presentable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidationEvidence {
    pub presentation_files: usize,
    pub ignored_files: usize,
    pub ignored_directories: usize,
    pub preflight_parse: bool,
}

pub fn find_presentations(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = std::fs::read_dir(folder)
        .map_err(|error| format!("unable to read {}: {error}", folder.display()))?;
    let mut presentations = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= MAX_FOLDER_ENTRIES {
            return Err(format!(
                "{} contains too many entries to inspect safely",
                folder.display()
            ));
        }
        let entry =
            entry.map_err(|error| format!("unable to enumerate {}: {error}", folder.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("unable to inspect {}: {error}", entry.path().display()))?;
        let name = entry.file_name();
        if file_type.is_file()
            && Path::new(&name)
                .extension()
                .is_some_and(|value| value == "pin")
        {
            presentations.push(entry.path());
            if presentations.len() > MAX_PRESENTATION_FILES {
                return Err(format!(
                    "{} contains more than {MAX_PRESENTATION_FILES} presentation files",
                    folder.display()
                ));
            }
        }
    }
    presentations.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    Ok(presentations)
}

fn settings() -> Option<gio::Settings> {
    let source = gio::SettingsSchemaSource::default()?;
    let schema = source.lookup(APP_ID, true)?;
    Some(gio::Settings::new_full(
        &schema,
        gio::SettingsBackend::NONE,
        None,
    ))
}

fn mark_welcome_complete(settings: Option<&gio::Settings>) {
    if let Some(settings) = settings {
        let _ = settings.set_boolean("welcome-complete", true);
    }
}

fn show_problem(parent: &adw::ApplicationWindow, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_response("close", "Close");
    dialog.set_close_response("close");
    dialog.present(Some(parent));
}

fn show_export_success(parent: &adw::ApplicationWindow, output: &Path) {
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("The presentation");
    let dialog = adw::AlertDialog::new(
        Some("PDF Exported"),
        Some(&format!("“{name}” was exported successfully.")),
    );
    dialog.add_response("close", "Close");
    dialog.set_close_response("close");
    dialog.present(Some(parent));
}

fn copy_introduction_to_folder(source: &Path, destination: &Path) -> Result<PathBuf, String> {
    let source_folder = source
        .parent()
        .ok_or_else(|| "the bundled introduction has no containing folder".to_owned())?;
    let mut destination_entries = std::fs::read_dir(destination)
        .map_err(|error| format!("unable to inspect {}: {error}", destination.display()))?;
    if destination_entries.next().is_some() {
        return Err("Choose an empty folder so existing work is not overwritten.".into());
    }

    fn copy_contents(source: &Path, destination: &Path) -> Result<(), String> {
        for entry in std::fs::read_dir(source)
            .map_err(|error| format!("unable to read {}: {error}", source.display()))?
        {
            let entry = entry.map_err(|error| error.to_string())?;
            let file_type = entry.file_type().map_err(|error| error.to_string())?;
            let target = destination.join(entry.file_name());
            if file_type.is_dir() {
                std::fs::create_dir(&target).map_err(|error| error.to_string())?;
                copy_contents(&entry.path(), &target)?;
            } else if file_type.is_file() {
                std::fs::copy(entry.path(), &target).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    copy_contents(source_folder, destination)?;
    let copied = destination.join(
        source
            .file_name()
            .ok_or_else(|| "the bundled introduction has no filename".to_owned())?,
    );
    copied
        .is_file()
        .then_some(copied)
        .ok_or_else(|| "the copied introduction is missing its presentation file".into())
}

fn choose_introduction_folder(
    parent: &adw::ApplicationWindow,
    introduction: PathBuf,
    ignore_comments: bool,
    open_in_editor: bool,
    edit: Rc<dyn Fn(PathBuf, bool, adw::ApplicationWindow)>,
) {
    let dialog = gtk::FileDialog::builder()
        .title(if open_in_editor {
            "Open Introduction in Editor"
        } else {
            "Save Editable Introduction"
        })
        .accept_label(if open_in_editor {
            "Open Copy in Editor"
        } else {
            "Save Copy Here"
        })
        .build();
    dialog.select_folder(
        Some(parent),
        gio::Cancellable::NONE,
        glib::clone!(
            #[weak]
            parent,
            #[strong]
            introduction,
            #[strong]
            edit,
            move |result| {
                let Ok(folder) = result else {
                    return;
                };
                let Some(folder) = folder.path() else {
                    show_problem(
                        &parent,
                        "Unsupported Folder",
                        "Choose a local folder provided by the document portal.",
                    );
                    return;
                };
                match copy_introduction_to_folder(&introduction, &folder) {
                    Ok(presentation) if open_in_editor => {
                        parent.set_visible(false);
                        edit(presentation, ignore_comments, parent.clone());
                    }
                    Ok(presentation) => show_problem(
                        &parent,
                        "Editable Introduction Saved",
                        &format!("Saved {}", presentation.display()),
                    ),
                    Err(error) => show_problem(&parent, "Unable to Save Introduction", &error),
                }
            }
        ),
    );
}

fn show_pdf_progress(
    parent: &adw::ApplicationWindow,
    source: PathBuf,
    output: PathBuf,
    ignore_comments: bool,
    options: pdf::Options,
) {
    let dialog = adw::Dialog::new();
    dialog.set_title("Exporting PDF");
    dialog.set_content_width(420);
    dialog.set_content_height(220);
    dialog.set_can_close(false);
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_start(24)
        .margin_end(24)
        .margin_top(24)
        .margin_bottom(24)
        .build();
    let label = gtk::Label::new(Some("Preparing presentation…"));
    label.set_wrap(true);
    label.set_justify(gtk::Justification::Center);
    let progress = gtk::ProgressBar::new();
    progress.set_show_text(true);
    progress.set_text(Some("Preparing…"));
    progress.set_accessible_role(gtk::AccessibleRole::ProgressBar);
    let cancel = gtk::Button::with_label("Cancel Export");
    cancel.set_halign(gtk::Align::End);
    content.append(&label);
    content.append(&progress);
    content.append(&cancel);
    dialog.set_child(Some(&content));

    let cancellable = gio::Cancellable::new();
    cancel.connect_clicked(glib::clone!(
        #[strong]
        cancellable,
        #[weak]
        label,
        move |button| {
            cancellable.cancel();
            label.set_text("Cancelling export…");
            button.set_label("Cancelling…");
            button.set_sensitive(false);
        }
    ));
    let completed_output = output.clone();
    pdf::export_async(
        source,
        output,
        ignore_comments,
        options,
        cancellable,
        glib::clone!(
            #[weak]
            progress,
            #[weak]
            label,
            move |completed, total| {
                let text = format!("{completed} of {total} slides");
                progress.set_fraction(completed as f64 / total.max(1) as f64);
                progress.set_text(Some(&text));
                label.set_text("Rendering slides and writing the PDF…");
            }
        ),
        glib::clone!(
            #[weak]
            dialog,
            #[weak]
            parent,
            #[strong]
            completed_output,
            move |result| {
                dialog.set_can_close(true);
                dialog.close();
                match result {
                    Ok(()) => show_export_success(&parent, &completed_output),
                    Err(error) if error == "PDF export cancelled" => {}
                    Err(error) => show_problem(&parent, "PDF Export Failed", &error),
                }
            }
        ),
    );
    dialog.present(Some(parent));
}

fn show_pdf_options(parent: &adw::ApplicationWindow, source: PathBuf, ignore_comments: bool) {
    let dialog = adw::Dialog::new();
    dialog.set_title("Export Presentation as PDF");
    dialog.set_content_width(460);
    dialog.set_content_height(360);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_start(24)
        .margin_end(24)
        .margin_top(24)
        .margin_bottom(24)
        .build();
    let heading = gtk::Label::new(Some("Choose the PDF output options."));
    heading.set_halign(gtk::Align::Start);
    heading.add_css_class("title-3");
    let page_size = gtk::DropDown::from_strings(&["A4", "Letter"]);
    page_size.set_selected(0);
    page_size.set_valign(gtk::Align::Center);
    let options_group = adw::PreferencesGroup::new();
    let page_size_row = adw::ActionRow::builder().title("Page Size").build();
    page_size_row.add_suffix(&page_size);
    page_size_row.set_activatable_widget(Some(&page_size));
    let orientation = gtk::DropDown::from_strings(&["Landscape", "Portrait"]);
    orientation.set_selected(0);
    orientation.set_valign(gtk::Align::Center);
    let orientation_row = adw::ActionRow::builder().title("Orientation").build();
    orientation_row.add_suffix(&orientation);
    orientation_row.set_activatable_widget(Some(&orientation));
    let notes = adw::SwitchRow::builder()
        .title("Speaker Notes")
        .subtitle("Include a notes page after each annotated slide")
        .active(true)
        .build();
    let cancel = gtk::Button::with_label("Cancel");
    cancel.set_hexpand(true);
    let export = gtk::Button::with_label("Choose Destination…");
    export.add_css_class("suggested-action");
    export.set_hexpand(true);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    actions.set_homogeneous(true);
    actions.append(&cancel);
    actions.append(&export);
    content.append(&heading);
    options_group.add(&page_size_row);
    options_group.add(&orientation_row);
    options_group.add(&notes);
    content.append(&options_group);
    content.append(&actions);
    dialog.set_child(Some(&content));

    cancel.connect_clicked(glib::clone!(
        #[weak]
        dialog,
        move |_| {
            dialog.close();
        }
    ));

    export.connect_clicked(glib::clone!(
        #[weak]
        dialog,
        #[weak]
        parent,
        #[strong]
        source,
        #[strong]
        page_size,
        #[strong]
        orientation,
        #[strong]
        notes,
        move |_| {
            let suggested = source
                .file_stem()
                .and_then(|name| name.to_str())
                .map(|name| format!("{name}.pdf"))
                .unwrap_or_else(|| "presentation.pdf".into());
            let file_dialog = gtk::FileDialog::builder()
                .title("Choose PDF Destination")
                .accept_label("Export")
                .initial_name(&suggested)
                .build();
            file_dialog.save(
                Some(&parent),
                None::<&gio::Cancellable>,
                glib::clone!(
                    #[weak]
                    dialog,
                    #[weak]
                    parent,
                    #[strong]
                    source,
                    #[strong]
                    page_size,
                    #[strong]
                    orientation,
                    #[strong]
                    notes,
                    move |result| {
                        let Ok(file) = result else {
                            return;
                        };
                        let Some(output) = file.path() else {
                            show_problem(
                                &parent,
                                "Unsupported Destination",
                                "Choose a local destination provided by the document portal.",
                            );
                            return;
                        };
                        let options = pdf::Options {
                            page_size: if page_size.selected() == 1 {
                                pdf::PageSize::Letter
                            } else {
                                pdf::PageSize::A4
                            },
                            orientation: if orientation.selected() == 1 {
                                pdf::Orientation::Portrait
                            } else {
                                pdf::Orientation::Landscape
                            },
                            include_speaker_notes: notes.is_active(),
                            asset_access: pinpoint_core::asset::Access::Confined,
                        };
                        dialog.close();
                        show_pdf_progress(
                            &parent,
                            source.clone(),
                            output,
                            ignore_comments,
                            options,
                        );
                    }
                ),
            );
        }
    ));
    dialog.present(Some(parent));
}

fn display_path(file: &gio::File, fallback: &Path) -> String {
    file.query_info(
        DOCUMENT_PORTAL_HOST_PATH_ATTRIBUTE,
        gio::FileQueryInfoFlags::NONE,
        gio::Cancellable::NONE,
    )
    .ok()
    .and_then(|info| {
        let path = info.attribute_string(DOCUMENT_PORTAL_HOST_PATH_ATTRIBUTE)?;
        (!path.is_empty()).then(|| path.to_string())
    })
    .or_else(|| {
        fallback
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
    })
    .unwrap_or_else(|| "Presentation".into())
}

fn monitor_name(monitor: &gdk::Monitor) -> String {
    monitor
        .connector()
        .or_else(|| monitor.model())
        .map_or_else(|| "Display".into(), |name| name.to_string())
}

fn audience_monitor_names(monitors: Option<&gio::ListModel>) -> Vec<String> {
    let mut names = vec!["Automatic".to_owned()];
    let Some(monitors) = monitors else {
        return names;
    };
    for index in 0..monitors.n_items() {
        if let Some(monitor) = monitors
            .item(index)
            .and_then(|monitor| monitor.downcast::<gdk::Monitor>().ok())
            .filter(gdk::Monitor::is_valid)
        {
            names.push(monitor_name(&monitor));
        }
    }
    names
}

fn audience_monitor_name(names: &[String], selected: u32) -> Option<String> {
    names
        .get(selected as usize)
        .filter(|name| name.as_str() != "Automatic")
        .cloned()
}

fn audience_monitor_choice(names: &[String], dropdown: &gtk::DropDown) -> Option<String> {
    audience_monitor_name(names, dropdown.selected())
}

fn audience_monitor_selection(previous: Option<&str>, names: &[String]) -> u32 {
    previous
        .and_then(|previous| names.iter().position(|name| name == previous))
        .unwrap_or(0) as u32
}

fn refresh_audience_monitor_choice(
    names: &Rc<RefCell<Vec<String>>>,
    dropdown: &gtk::DropDown,
    monitors: Option<&gio::ListModel>,
) {
    let selected = audience_monitor_choice(&names.borrow(), dropdown);
    let refreshed_names = audience_monitor_names(monitors);
    let selection = audience_monitor_selection(selected.as_deref(), &refreshed_names);
    let items = refreshed_names
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    dropdown.set_model(Some(&gtk::StringList::new(&items)));
    dropdown.set_selected(selection);
    dropdown.set_sensitive(refreshed_names.len() > 1);
    *names.borrow_mut() = refreshed_names;
}

fn presentation_details(presentation: &Presentation) -> String {
    let slides = presentation.slides.len();
    let mut details = format!("{slides} {}", if slides == 1 { "slide" } else { "slides" });
    let visual_assets = presentation
        .slides
        .iter()
        .filter(|slide| {
            matches!(
                slide.background_type,
                BackgroundType::Image | BackgroundType::Svg
            )
        })
        .count();
    let videos = presentation
        .slides
        .iter()
        .filter(|slide| slide.background_type == BackgroundType::Video)
        .count();
    let notes = presentation
        .slides
        .iter()
        .filter(|slide| {
            slide
                .speaker_notes
                .as_deref()
                .is_some_and(|notes| !notes.is_empty())
        })
        .count();
    if visual_assets > 0 {
        details.push_str(&format!(
            " · {visual_assets} visual {}",
            if visual_assets == 1 {
                "asset"
            } else {
                "assets"
            }
        ));
    }
    if videos > 0 {
        details.push_str(&format!(
            " · {videos} {}",
            if videos == 1 { "video" } else { "videos" }
        ));
    }
    if notes > 0 {
        details.push_str(" · speaker notes");
    }
    details
}

fn selection_from_file(
    file: &gio::File,
    ignore_comments: bool,
) -> Result<PresentationSelection, String> {
    let path = file.path().ok_or_else(|| {
        "The selected presentation is not available as a local portal document.".to_owned()
    })?;
    let label = display_path(file, &path);
    let (details, presentable) = match presentation::load(&path, ignore_comments) {
        Ok(presentation) => (presentation_details(&presentation), true),
        Err(error) => (format!("Cannot open: {error}"), false),
    };
    Ok(PresentationSelection {
        path,
        label,
        details,
        presentable,
    })
}

fn show_presentation_choice(
    parent: &adw::ApplicationWindow,
    files: Vec<PathBuf>,
    selected: Rc<dyn Fn(gio::File)>,
) {
    let dialog = adw::Dialog::new();
    dialog.set_title("Choose a Presentation");
    dialog.set_content_width(520);
    dialog.set_content_height((180 + files.len() as i32 * 56).min(560));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    let group = adw::PreferencesGroup::builder()
        .description("This folder contains more than one .pin file.")
        .build();
    group.set_margin_start(18);
    group.set_margin_end(18);
    group.set_margin_top(18);
    group.set_margin_bottom(18);
    for file in files {
        let file = gio::File::for_path(file);
        let name = display_path(&file, &file.path().unwrap_or_default());
        let row = adw::ActionRow::builder().title(name).build();
        let choose = gtk::Button::from_icon_name("go-next-symbolic");
        choose.add_css_class("flat");
        choose.set_valign(gtk::Align::Center);
        choose.set_tooltip_text(Some("Open Presentation"));
        choose.update_property(&[gtk::accessible::Property::Label("Open Presentation")]);
        row.add_suffix(&choose);
        row.set_activatable_widget(Some(&choose));
        choose.connect_clicked(glib::clone!(
            #[weak]
            dialog,
            #[strong]
            file,
            #[strong]
            selected,
            move |_| {
                selected(file.clone());
                dialog.close();
            }
        ));
        group.add(&row);
    }
    scrolled.set_child(Some(&group));
    toolbar.set_content(Some(&scrolled));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

#[allow(clippy::too_many_arguments)]
pub fn build(
    app: &adw::Application,
    initial_fullscreen: bool,
    initial_speaker_mode: bool,
    initial_ignore_comments: bool,
    introduction: PathBuf,
    start: impl Fn(LaunchOptions) + 'static,
    edit: impl Fn(PathBuf, bool, adw::ApplicationWindow) + 'static,
    new_presentation: impl Fn(adw::ApplicationWindow) + 'static,
) {
    let settings = settings();
    let selected = Rc::new(RefCell::new(None::<PresentationSelection>));
    let start: Rc<dyn Fn(LaunchOptions)> = Rc::new(start);
    let edit: Rc<dyn Fn(PathBuf, bool, adw::ApplicationWindow)> = Rc::new(edit);
    let new_presentation: Rc<dyn Fn(adw::ApplicationWindow)> = Rc::new(new_presentation);

    let header = adw::HeaderBar::new();
    let back = gtk::Button::from_icon_name("go-previous-symbolic");
    back.set_tooltip_text(Some("Back to presentation selection"));
    back.update_property(&[gtk::accessible::Property::Label(
        "Back to presentation selection",
    )]);
    back.set_visible(false);
    header.pack_start(&back);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.set_margin_top(12);
    content.set_margin_bottom(24);
    let clamp = adw::Clamp::builder()
        .maximum_size(680)
        .child(&content)
        .build();
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&clamp)
        .build();
    toolbar.set_content(Some(&scrolled));

    let hero = gtk::Box::new(gtk::Orientation::Vertical, 6);
    hero.set_halign(gtk::Align::Center);
    hero.set_margin_top(2);
    let icon = gtk::Image::from_icon_name(APP_ID);
    icon.set_pixel_size(48);
    icon.add_css_class("accent");
    let title = gtk::Label::new(Some("Open a Presentation"));
    title.add_css_class("title-2");
    let description = gtk::Label::new(Some("Excellent presentations for hackers."));
    description.add_css_class("dimmed");
    description.set_wrap(true);
    description.set_justify(gtk::Justification::Center);
    hero.append(&icon);
    hero.append(&title);
    hero.append(&description);
    content.append(&hero);

    let launch_group = adw::PreferencesGroup::builder().title("Start").build();
    let open_row = adw::ActionRow::builder()
        .title("Open Presentation…")
        .subtitle("Choose a folder containing a .pin file and its assets")
        .build();
    let open_icon = gtk::Image::from_icon_name("folder-open-symbolic");
    open_icon.add_css_class("dimmed");
    let open = gtk::Button::from_icon_name("go-next-symbolic");
    open.add_css_class("flat");
    open.set_valign(gtk::Align::Center);
    open.set_tooltip_text(Some("Open Presentation"));
    open.update_property(&[gtk::accessible::Property::Label("Open Presentation")]);
    open_row.add_prefix(&open_icon);
    open_row.add_suffix(&open);
    open_row.set_activatable_widget(Some(&open));
    launch_group.add(&open_row);

    let create_row = adw::ActionRow::builder()
        .title("New Presentation")
        .subtitle("Start with a blank slide in the composition editor")
        .build();
    let create_icon = gtk::Image::from_icon_name("document-new-symbolic");
    create_icon.add_css_class("dimmed");
    let create = gtk::Button::from_icon_name("go-next-symbolic");
    create.add_css_class("flat");
    create.set_valign(gtk::Align::Center);
    create.set_tooltip_text(Some("New Presentation"));
    create.update_property(&[gtk::accessible::Property::Label("New Presentation")]);
    create_row.add_prefix(&create_icon);
    create_row.add_suffix(&create);
    create_row.set_activatable_widget(Some(&create));
    launch_group.add(&create_row);
    content.append(&launch_group);

    let introduction_group = adw::PreferencesGroup::builder()
        .title("Learn Pinpoint")
        .build();
    let introduction_row = adw::ActionRow::builder()
        .title("Introduction")
        .subtitle("A short tour made with Pinpoint")
        .build();
    let introduction_icon = gtk::Image::from_icon_name("help-contents-symbolic");
    introduction_icon.add_css_class("dimmed");
    let view_introduction = gtk::Button::from_icon_name("media-playback-start-symbolic");
    view_introduction.add_css_class("flat");
    view_introduction.set_valign(gtk::Align::Center);
    view_introduction.set_tooltip_text(Some("View Introduction"));
    view_introduction.update_property(&[gtk::accessible::Property::Label("View Introduction")]);
    let save_introduction = gtk::Button::from_icon_name("document-save-symbolic");
    save_introduction.add_css_class("flat");
    save_introduction.set_valign(gtk::Align::Center);
    save_introduction.set_tooltip_text(Some("Save an Editable Copy"));
    save_introduction.update_property(&[gtk::accessible::Property::Label("Save an Editable Copy")]);
    introduction_row.add_prefix(&introduction_icon);
    introduction_row.add_suffix(&view_introduction);
    introduction_row.add_suffix(&save_introduction);
    introduction_row.set_activatable_widget(Some(&view_introduction));
    introduction_group.add(&introduction_row);
    let edit_introduction_row = adw::ActionRow::builder()
        .title("Make an Editable Copy")
        .subtitle("Copy the introduction and open it in the composition editor")
        .build();
    let edit_introduction = gtk::Button::from_icon_name("go-next-symbolic");
    edit_introduction.add_css_class("flat");
    edit_introduction.set_valign(gtk::Align::Center);
    edit_introduction.set_tooltip_text(Some("Make an Editable Copy"));
    edit_introduction.update_property(&[gtk::accessible::Property::Label("Make an Editable Copy")]);
    edit_introduction_row.add_suffix(&edit_introduction);
    edit_introduction_row.set_activatable_widget(Some(&edit_introduction));
    introduction_group.add(&edit_introduction_row);
    let selected_section = gtk::Box::new(gtk::Orientation::Vertical, 18);
    selected_section.set_visible(false);
    let selected_hero = gtk::Box::new(gtk::Orientation::Vertical, 6);
    selected_hero.set_halign(gtk::Align::Center);
    selected_hero.set_margin_top(18);
    selected_hero.set_margin_bottom(8);
    let selected_icon = gtk::Image::from_icon_name("x-office-presentation-symbolic");
    selected_icon.set_pixel_size(64);
    selected_icon.add_css_class("accent");
    let selected_context = gtk::Label::new(Some("Ready to Present"));
    selected_context.add_css_class("title-4");
    selected_context.add_css_class("accent");
    let selected_title = gtk::Label::new(Some("No presentation selected"));
    selected_title.add_css_class("title-1");
    selected_title.set_wrap(true);
    selected_title.set_justify(gtk::Justification::Center);
    selected_title.set_max_width_chars(34);
    let selected_details = gtk::Label::new(None);
    selected_details.add_css_class("dimmed");
    selected_details.set_wrap(true);
    selected_details.set_justify(gtk::Justification::Center);
    selected_details.set_max_width_chars(52);
    selected_hero.append(&selected_icon);
    selected_hero.append(&selected_context);
    selected_hero.append(&selected_title);
    selected_hero.append(&selected_details);

    let options_group = adw::PreferencesGroup::new();
    options_group.set_visible(false);
    let options_expander = adw::ExpanderRow::builder()
        .title("Presentation Options")
        .subtitle("Fullscreen, speaker view, comments and display")
        .build();
    let monitor_model = gdk::Display::default().map(|display| display.monitors());
    let audience_monitor_names =
        Rc::new(RefCell::new(audience_monitor_names(monitor_model.as_ref())));
    let audience_monitor_names_ref = audience_monitor_names.borrow();
    let audience_monitor_items = audience_monitor_names_ref
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let audience_monitor = gtk::DropDown::from_strings(&audience_monitor_items);
    audience_monitor.set_sensitive(audience_monitor_names.borrow().len() > 1);
    audience_monitor.set_valign(gtk::Align::Center);
    let audience_monitor_row = adw::ActionRow::builder()
        .title("Audience Display")
        .subtitle("Choose where fullscreen slides appear")
        .build();
    audience_monitor_row.add_suffix(&audience_monitor);
    audience_monitor_row.set_activatable_widget(Some(&audience_monitor));
    let fullscreen = adw::SwitchRow::builder()
        .title("Start Fullscreen")
        .subtitle("Use the audience display for the presentation")
        .active(initial_fullscreen)
        .build();
    let speaker = adw::SwitchRow::builder()
        .title("Show Speaker View")
        .subtitle("Show notes, previews, timing and controls")
        .active(initial_speaker_mode)
        .build();
    let ignore_comments = adw::SwitchRow::builder()
        .title("Ignore Comments")
        .subtitle("Do not display comment lines as speaker notes")
        .active(initial_ignore_comments)
        .build();
    options_expander.add_row(&fullscreen);
    options_expander.add_row(&speaker);
    options_expander.add_row(&ignore_comments);
    options_expander.add_row(&audience_monitor_row);
    options_group.add(&options_expander);
    if let Some(monitor_model) = monitor_model {
        monitor_model.connect_items_changed(glib::clone!(
            #[weak]
            audience_monitor,
            #[strong]
            audience_monitor_names,
            move |monitors, _, _, _| {
                refresh_audience_monitor_choice(
                    &audience_monitor_names,
                    &audience_monitor,
                    Some(monitors),
                );
            }
        ));
    }

    let selected_actions = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    selected_actions.set_homogeneous(true);
    selected_actions.set_halign(gtk::Align::Fill);
    let present = gtk::Button::with_label("Present");
    present.add_css_class("suggested-action");
    present.set_hexpand(true);
    present.set_height_request(52);
    present.set_tooltip_text(Some("Present this deck (Ctrl+P)"));
    present.update_property(&[gtk::accessible::Property::KeyShortcuts("Ctrl+P")]);
    present.set_sensitive(false);
    let rehearse = gtk::Button::with_label("Rehearse");
    rehearse.set_hexpand(true);
    rehearse.set_height_request(52);
    rehearse.set_tooltip_text(Some("Rehearse and record slide timings (Ctrl+Shift+R)"));
    rehearse.update_property(&[gtk::accessible::Property::KeyShortcuts("Ctrl+Shift+R")]);
    rehearse.set_sensitive(false);
    let edit_selected = gtk::Button::from_icon_name("go-next-symbolic");
    edit_selected.add_css_class("flat");
    edit_selected.set_valign(gtk::Align::Center);
    edit_selected.set_tooltip_text(Some("Edit Presentation"));
    edit_selected.update_property(&[gtk::accessible::Property::Label("Edit Presentation")]);
    edit_selected.set_sensitive(false);
    let export_selected = gtk::Button::from_icon_name("go-next-symbolic");
    export_selected.add_css_class("flat");
    export_selected.set_valign(gtk::Align::Center);
    export_selected.set_tooltip_text(Some("Export as PDF"));
    export_selected.update_property(&[gtk::accessible::Property::Label("Export as PDF")]);
    export_selected.set_sensitive(false);
    selected_actions.append(&present);
    selected_actions.append(&rehearse);

    let selected_file_actions = adw::PreferencesGroup::builder()
        .title("Work with Presentation")
        .build();
    let edit_row = adw::ActionRow::builder()
        .title("Edit Presentation")
        .subtitle("Open the source and live preview")
        .build();
    let edit_icon = gtk::Image::from_icon_name("document-edit-symbolic");
    edit_icon.add_css_class("dimmed");
    edit_row.add_prefix(&edit_icon);
    edit_row.add_suffix(&edit_selected);
    edit_row.set_activatable_widget(Some(&edit_selected));
    selected_file_actions.add(&edit_row);
    let export_row = adw::ActionRow::builder()
        .title("Export as PDF")
        .subtitle("Create a portable copy of this presentation")
        .build();
    let export_icon = gtk::Image::from_icon_name("document-save-symbolic");
    export_icon.add_css_class("dimmed");
    export_row.add_prefix(&export_icon);
    export_row.add_suffix(&export_selected);
    export_row.set_activatable_widget(Some(&export_selected));
    selected_file_actions.add(&export_row);

    selected_section.append(&selected_hero);
    selected_section.append(&selected_actions);
    selected_section.append(&options_group);
    selected_section.append(&selected_file_actions);
    content.append(&selected_section);
    content.append(&introduction_group);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Pinpoint")
        .default_width(760)
        .default_height(760)
        .content(&toolbar)
        .build();
    app_shell::add_application_menu(&window, &header);

    let shortcuts = gtk::EventControllerKey::new();
    shortcuts.connect_key_pressed(glib::clone!(
        #[weak]
        open,
        #[weak]
        create,
        #[weak]
        present,
        #[weak]
        rehearse,
        #[weak]
        export_selected,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, modifiers| {
            if !modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }
            let button = match key {
                gdk::Key::o | gdk::Key::O => &open,
                gdk::Key::n | gdk::Key::N => &create,
                gdk::Key::p | gdk::Key::P => &present,
                gdk::Key::e | gdk::Key::E => &export_selected,
                gdk::Key::r | gdk::Key::R if modifiers.contains(gdk::ModifierType::SHIFT_MASK) => {
                    &rehearse
                }
                _ => return glib::Propagation::Proceed,
            };
            if button.is_sensitive() && button.is_visible() {
                button.emit_clicked();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    ));
    window.add_controller(shortcuts);

    let select: Rc<dyn Fn(gio::File)> = Rc::new(glib::clone!(
        #[weak]
        window,
        #[weak]
        selected_section,
        #[weak]
        selected_title,
        #[weak]
        selected_details,
        #[weak]
        selected_icon,
        #[weak]
        selected_context,
        #[weak]
        present,
        #[weak]
        rehearse,
        #[weak]
        edit_selected,
        #[weak]
        export_selected,
        #[weak]
        ignore_comments,
        #[weak]
        options_group,
        #[weak]
        hero,
        #[weak]
        launch_group,
        #[weak]
        introduction_group,
        #[weak]
        back,
        #[strong]
        selected,
        #[strong]
        settings,
        move |file: gio::File| {
            let selection = match selection_from_file(&file, ignore_comments.is_active()) {
                Ok(selection) => selection,
                Err(error) => {
                    show_problem(&window, "Unable to Open Presentation", &error);
                    return;
                }
            };
            selected_title.set_label(&selection.label);
            selected_details.set_label(&selection.details);
            if selection.presentable {
                selected_icon.set_icon_name(Some("x-office-presentation-symbolic"));
                selected_context.set_label("Ready to Present");
                selected_details.remove_css_class("error");
            } else {
                selected_icon.set_icon_name(Some("dialog-warning-symbolic"));
                selected_context.set_label("Needs Attention");
                selected_details.add_css_class("error");
            }
            selected_section.set_visible(true);
            present.set_sensitive(selection.presentable);
            rehearse.set_sensitive(selection.presentable);
            edit_selected.set_sensitive(true);
            export_selected.set_sensitive(selection.presentable);
            options_group.set_visible(selection.presentable);
            hero.set_visible(false);
            launch_group.set_visible(false);
            introduction_group.set_visible(false);
            back.set_visible(true);
            window.set_default_widget(selection.presentable.then_some(&present));
            *selected.borrow_mut() = Some(selection);
            mark_welcome_complete(settings.as_ref());
        }
    ));

    back.connect_clicked(glib::clone!(
        #[weak]
        selected_section,
        #[weak]
        selected_title,
        #[weak]
        selected_details,
        #[weak]
        selected_icon,
        #[weak]
        selected_context,
        #[weak]
        present,
        #[weak]
        rehearse,
        #[weak]
        edit_selected,
        #[weak]
        export_selected,
        #[weak]
        options_group,
        #[weak]
        hero,
        #[weak]
        launch_group,
        #[weak]
        introduction_group,
        #[weak]
        window,
        #[weak]
        back,
        #[strong]
        selected,
        move |_| {
            *selected.borrow_mut() = None;
            selected_section.set_visible(false);
            selected_title.set_label("No presentation selected");
            selected_details.set_label("");
            selected_details.remove_css_class("error");
            selected_icon.set_icon_name(Some("x-office-presentation-symbolic"));
            selected_context.set_label("Ready to Present");
            present.set_sensitive(false);
            rehearse.set_sensitive(false);
            edit_selected.set_sensitive(false);
            export_selected.set_sensitive(false);
            options_group.set_visible(false);
            hero.set_visible(true);
            launch_group.set_visible(true);
            introduction_group.set_visible(true);
            back.set_visible(false);
            window.set_default_widget(gtk::Widget::NONE);
        }
    ));

    ignore_comments.connect_active_notify(glib::clone!(
        #[strong]
        selected,
        #[strong]
        select,
        move |_| {
            let Some(path) = selected
                .borrow()
                .as_ref()
                .map(|selection| selection.path.clone())
            else {
                return;
            };
            select(gio::File::for_path(path));
        }
    ));

    open.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[strong]
        select,
        move |_| {
            let dialog = gtk::FileDialog::builder()
                .title("Choose Presentation Folder")
                .accept_label("Choose Folder")
                .build();
            dialog.select_folder(Some(&window), gio::Cancellable::NONE, glib::clone!(
                #[weak]
                window,
                #[strong]
                select,
                move |result| match result {
                    Ok(folder) => {
                        let Some(path) = folder.path() else {
                            show_problem(
                                &window,
                                "Unsupported Folder",
                                "The selected folder is not available as a local portal document.",
                            );
                            return;
                        };
                        match find_presentations(&path) {
                            Ok(files) if files.is_empty() => show_problem(
                                &window,
                                "No Pinpoint Presentations Found",
                                "Choose a folder containing at least one .pin file.",
                            ),
                            Ok(mut files) if files.len() == 1 => {
                                select(gio::File::for_path(files.remove(0)))
                            }
                            Ok(files) => show_presentation_choice(&window, files, select.clone()),
                            Err(error) => {
                                show_problem(&window, "Unable to Read Folder", &error)
                            }
                        }
                    }
                    Err(error)
                        if error.matches(gtk::DialogError::Cancelled)
                            || error.matches(gtk::DialogError::Dismissed)
                            || error.matches(gio::IOErrorEnum::Cancelled) => {}
                    Err(error) => {
                        show_problem(&window, "Unable to Open Folder", &error.to_string())
                    }
                }
            ));
        }
    ));

    create.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[strong]
        new_presentation,
        move |_| {
            window.set_visible(false);
            new_presentation(window.clone());
        }
    ));

    present.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        fullscreen,
        #[weak]
        speaker,
        #[weak]
        ignore_comments,
        #[weak]
        audience_monitor,
        #[strong]
        selected,
        #[strong]
        audience_monitor_names,
        #[strong]
        start,
        move |_| {
            let Some(presentation) = selected.borrow().clone() else {
                return;
            };
            let options = LaunchOptions {
                presentation: presentation.path,
                presentation_label: presentation.label,
                fullscreen: fullscreen.is_active(),
                speaker_mode: speaker.is_active(),
                rehearse: false,
                audience_monitor: audience_monitor_choice(
                    &audience_monitor_names.borrow(),
                    &audience_monitor,
                ),
                ignore_comments: ignore_comments.is_active(),
            };
            window.close();
            start(options);
        }
    ));

    rehearse.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        fullscreen,
        #[weak]
        ignore_comments,
        #[weak]
        audience_monitor,
        #[strong]
        selected,
        #[strong]
        audience_monitor_names,
        #[strong]
        start,
        move |_| {
            let Some(presentation) = selected.borrow().clone() else {
                return;
            };
            let options = LaunchOptions {
                presentation: presentation.path,
                presentation_label: presentation.label,
                fullscreen: fullscreen.is_active(),
                speaker_mode: true,
                rehearse: true,
                audience_monitor: audience_monitor_choice(
                    &audience_monitor_names.borrow(),
                    &audience_monitor,
                ),
                ignore_comments: ignore_comments.is_active(),
            };
            window.close();
            start(options);
        }
    ));

    edit_selected.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        ignore_comments,
        #[strong]
        selected,
        #[strong]
        edit,
        move |_| {
            let Some(presentation) = selected.borrow().clone() else {
                return;
            };
            window.set_visible(false);
            edit(
                presentation.path,
                ignore_comments.is_active(),
                window.clone(),
            );
        }
    ));

    export_selected.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        ignore_comments,
        #[strong]
        selected,
        move |_| {
            let Some(presentation) = selected.borrow().clone() else {
                return;
            };
            show_pdf_options(&window, presentation.path, ignore_comments.is_active());
        }
    ));

    view_introduction.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        fullscreen,
        #[weak]
        speaker,
        #[weak]
        ignore_comments,
        #[weak]
        audience_monitor,
        #[strong]
        introduction,
        #[strong]
        audience_monitor_names,
        #[strong]
        settings,
        #[strong]
        start,
        move |_| {
            mark_welcome_complete(settings.as_ref());
            let options = LaunchOptions {
                presentation: introduction.clone(),
                presentation_label: "Introduction".into(),
                fullscreen: fullscreen.is_active(),
                speaker_mode: speaker.is_active(),
                rehearse: false,
                audience_monitor: audience_monitor_choice(
                    &audience_monitor_names.borrow(),
                    &audience_monitor,
                ),
                ignore_comments: ignore_comments.is_active(),
            };
            window.close();
            start(options);
        }
    ));

    save_introduction.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        ignore_comments,
        #[strong]
        introduction,
        #[strong]
        edit,
        move |_| {
            choose_introduction_folder(
                &window,
                introduction.clone(),
                ignore_comments.is_active(),
                false,
                edit.clone(),
            );
        }
    ));

    edit_introduction.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        ignore_comments,
        #[strong]
        introduction,
        #[strong]
        edit,
        move |_| {
            choose_introduction_folder(
                &window,
                introduction.clone(),
                ignore_comments.is_active(),
                true,
                edit.clone(),
            );
        }
    ));

    window.present();
}

pub fn validate() -> Result<ValidationEvidence, String> {
    let directory =
        std::env::temp_dir().join(format!("pinpoint-setup-validation-{}", std::process::id()));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let result = (|| {
        std::fs::write(directory.join("b.pin"), "--\nB\n").map_err(|error| error.to_string())?;
        std::fs::write(directory.join("a.pin"), "--\nA\n").map_err(|error| error.to_string())?;
        std::fs::write(directory.join("ignored.txt"), "ignored")
            .map_err(|error| error.to_string())?;
        std::fs::create_dir(directory.join("directory.pin")).map_err(|error| error.to_string())?;
        let files = find_presentations(&directory)?;
        let names = files
            .iter()
            .filter_map(|file| file.file_name().and_then(|name| name.to_str()))
            .collect::<Vec<_>>();
        if names != ["a.pin", "b.pin"] {
            return Err(format!("folder results were not sorted: {names:?}"));
        }
        if find_presentations(&directory.join("a.pin")).is_ok() {
            return Err("a regular file was accepted as a presentation folder".into());
        }
        let valid = selection_from_file(&gio::File::for_path(directory.join("a.pin")), false)?;
        if !valid.presentable || valid.label != "a.pin" || valid.details != "1 slide" {
            return Err("a valid selected presentation was not parsed for setup".into());
        }
        let broken = directory.join("broken.pin");
        std::fs::write(&broken, "[invalid\n").map_err(|error| error.to_string())?;
        let invalid = selection_from_file(&gio::File::for_path(&broken), false)?;
        if invalid.presentable || !invalid.details.starts_with("Cannot open: ") {
            return Err("an invalid selected presentation was marked presentable".into());
        }
        let commented = directory.join("commented.pin");
        std::fs::write(&commented, "--\nComment handling\n#Presenter reminder\n")
            .map_err(|error| error.to_string())?;
        let with_comments = selection_from_file(&gio::File::for_path(&commented), false)?;
        let without_comments = selection_from_file(&gio::File::for_path(&commented), true)?;
        if !with_comments.details.ends_with("speaker notes")
            || without_comments.details.ends_with("speaker notes")
        {
            return Err("comment handling did not refresh the presentation summary".into());
        }
        Ok(ValidationEvidence {
            presentation_files: files.len(),
            ignored_files: 1,
            ignored_directories: 1,
            preflight_parse: true,
        })
    })();
    let cleanup = std::fs::remove_dir_all(&directory);
    match (result, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(format!("unable to remove validation fixture: {error}")),
        (Ok(evidence), Ok(())) => Ok(evidence),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_contract_is_sorted_and_bounded_to_regular_pin_files() {
        let evidence = validate().expect("setup folder contract");
        assert_eq!(evidence.presentation_files, 2);
        assert_eq!(evidence.ignored_files, 1);
        assert_eq!(evidence.ignored_directories, 1);
    }

    #[test]
    fn parsed_details_match_the_c_setup_summary() {
        let presentation = presentation::parse(
            "-- [photo.jpg]\nFirst\n#Remember this\n-- [clip.mp4]\nSecond\n-- [logo.svg]\nThird\n",
            None,
            false,
        )
        .expect("valid presentation");
        assert_eq!(
            presentation_details(&presentation),
            "3 slides · 2 visual assets · 1 video · speaker notes"
        );
    }

    #[test]
    fn editable_introduction_copy_requires_an_empty_destination() {
        let root =
            std::env::temp_dir().join(format!("pinpoint-introduction-copy-{}", std::process::id()));
        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::create_dir_all(source.join("assets")).expect("source directory");
        std::fs::create_dir_all(&destination).expect("destination directory");
        let presentation = source.join("introduction.pin");
        std::fs::write(&presentation, "--\nIntroduction\n").expect("presentation source");
        std::fs::write(source.join("assets/logo.svg"), "<svg/>").expect("asset source");

        let copied =
            copy_introduction_to_folder(&presentation, &destination).expect("copy introduction");
        assert_eq!(
            std::fs::read_to_string(copied).expect("copied presentation"),
            "--\nIntroduction\n"
        );
        assert_eq!(
            std::fs::read_to_string(destination.join("assets/logo.svg")).expect("copied asset"),
            "<svg/>"
        );
        assert!(
            copy_introduction_to_folder(&presentation, &destination)
                .expect_err("non-empty destination must be rejected")
                .contains("empty folder")
        );
        std::fs::remove_dir_all(&root).expect("cleanup copy fixture");
    }

    #[test]
    fn audience_monitor_selection_keeps_automatic_as_the_default() {
        let names = vec!["Automatic".into(), "eDP-1".into(), "HDMI-A-1".into()];
        assert_eq!(audience_monitor_name(&names, 0), None);
        assert_eq!(audience_monitor_name(&names, 1), Some("eDP-1".into()));
        assert_eq!(audience_monitor_name(&names, 3), None);
    }

    #[test]
    fn audience_monitor_refresh_preserves_a_connected_display_or_falls_back() {
        let connected = vec!["Automatic".into(), "eDP-1".into(), "HDMI-A-1".into()];
        assert_eq!(audience_monitor_selection(Some("HDMI-A-1"), &connected), 2);

        let disconnected = vec!["Automatic".into(), "eDP-1".into()];
        assert_eq!(
            audience_monitor_selection(Some("HDMI-A-1"), &disconnected),
            0
        );
    }
}
