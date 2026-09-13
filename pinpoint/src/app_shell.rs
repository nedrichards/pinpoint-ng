use adw::prelude::*;
use gtk::{gdk, gio, glib};

const APP_ID: &str = "com.nedrichards.pinpoint";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const PROJECT_URI: &str = "https://github.com/nedrichards/pinpoint-ng";
const DOCUMENTATION_BASE_URI: &str = "https://github.com/nedrichards/pinpoint-ng/blob/main/docs/";
const DOCUMENTATION: [(&str, &str, &str); 4] = [
    ("format", "Presentation Format", "presentation-format.md"),
    (
        "command-line",
        "Command Line and Flatpak",
        "command-line.md",
    ),
    ("editors", "External Editors", "external-editors.md"),
    ("accessibility", "Accessibility", "accessibility.md"),
];

pub fn validate_menu_contract() -> Result<(), String> {
    let mut actions = std::collections::HashSet::new();
    for (action, label, document) in DOCUMENTATION {
        if action.is_empty() || label.is_empty() || !document.ends_with(".md") {
            return Err("application documentation menu contains an invalid item".into());
        }
        if !actions.insert(action) {
            return Err(format!("application menu repeats the {action} action"));
        }
    }
    if !PROJECT_URI.starts_with("https://") || !DOCUMENTATION_BASE_URI.starts_with("https://") {
        return Err("application links are not HTTPS".into());
    }
    Ok(())
}

fn add_uri_action(window: &adw::ApplicationWindow, name: &str, uri: String) {
    let action = gio::SimpleAction::new(name, None);
    action.connect_activate(glib::clone!(
        #[weak]
        window,
        move |_, _| {
            let launcher = gtk::UriLauncher::builder().uri(&uri).build();
            launcher.launch(
                Some(&window),
                None::<&gio::Cancellable>,
                glib::clone!(
                    #[weak]
                    window,
                    move |result| {
                        if let Err(error) = result {
                            let dialog = adw::AlertDialog::new(
                                Some("Unable to Open Documentation"),
                                Some(&error.to_string()),
                            );
                            dialog.add_response("close", "Close");
                            dialog.set_close_response("close");
                            dialog.present(Some(&window));
                        }
                    }
                ),
            );
        }
    ));
    window.add_action(&action);
}

pub fn add_application_menu(window: &adw::ApplicationWindow, header: &adw::HeaderBar) {
    let about = gio::SimpleAction::new("about", None);
    about.connect_activate(glib::clone!(
        #[weak]
        window,
        move |_, _| {
            let dialog = adw::AboutDialog::new();
            dialog.set_application_name("Pinpoint");
            dialog.set_application_icon(APP_ID);
            dialog.set_developer_name("Nick Richards");
            dialog.set_version(VERSION);
            dialog.set_comments(
                "Excellent presentations for hackers. Write concise plain-text \
                 slides in the editor of your choice.",
            );
            dialog.set_website(PROJECT_URI);
            dialog.set_issue_url("https://github.com/nedrichards/pinpoint-ng/issues");
            dialog.set_copyright("Copyright © 2026 Nick Richards");
            dialog.set_license_type(gtk::License::Lgpl21);
            dialog.add_legal_section(
                "Original Pinpoint codebase (inspiration)",
                Some("Copyright © 2010 Intel Corporation and the original Pinpoint contributors"),
                gtk::License::Lgpl21,
                None,
            );
            dialog.add_legal_section(
                "Big Buck Bunny excerpt",
                Some("Copyright © 2008 Blender Foundation"),
                gtk::License::Custom,
                Some(
                    "Creative Commons Attribution 3.0\n\n\
                     Source: https://commons.wikimedia.org/wiki/File:Big_Buck_Bunny_medium.ogv\n\
                     Attribution: https://www.bigbuckbunny.org/\n\
                     License: https://creativecommons.org/licenses/by/3.0/\n\n\
                     The bundled introduction uses a short excerpt, trimmed and re-encoded as VP9/Opus WebM.",
                ),
            );
            dialog.add_credit_section(
                Some("Original Pinpoint Authors (inspiration)"),
                &[
                    "Øyvind Kolås",
                    "Damien Lespiau",
                    "Emmanuele Bassi",
                    "Neil Roberts",
                    "Nick Richards",
                    "Daniel G. Siegel",
                    "Jussi Kukkonen",
                    "Chris Lord",
                    "Will Thompson",
                    "Andoni Morales Alastruey",
                    "Vladimír Kincl",
                    "Antonio Terceiro",
                    "Gary Ching-Pang Lin",
                    "Lionel Landwerlin",
                    "Christoph Fischer",
                    "Douglas Bagnall",
                ],
            );
            dialog.present(Some(&window));
        }
    ));
    window.add_action(&about);

    let shortcuts = gio::SimpleAction::new("shortcuts", None);
    shortcuts.connect_activate(glib::clone!(
        #[weak]
        window,
        move |_, _| {
            show_shortcuts(&window);
        }
    ));
    window.add_action(&shortcuts);
    install_shortcut_access(window);

    for (action, _, document) in DOCUMENTATION {
        let name = format!("documentation-{action}");
        let uri = format!("{DOCUMENTATION_BASE_URI}{document}");
        add_uri_action(window, &name, uri);
    }

    let menu = gio::Menu::new();
    menu.append(Some("Keyboard Shortcuts"), Some("win.shortcuts"));
    for (action, label, _) in DOCUMENTATION {
        menu.append(Some(label), Some(&format!("win.documentation-{action}")));
    }
    menu.append(Some("About Pinpoint"), Some("win.about"));

    let button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Application Menu")
        .menu_model(&menu)
        .build();
    button.update_property(&[gtk::accessible::Property::Label("Application Menu")]);
    header.pack_end(&button);
}

fn show_shortcuts(window: &adw::ApplicationWindow) {
    let dialog = adw::Dialog::new();
    dialog.set_title("Keyboard Shortcuts");
    dialog.set_content_width(600);
    dialog.set_content_height(660);

    let header = adw::HeaderBar::builder()
        .title_widget(&adw::WindowTitle::new("Keyboard Shortcuts", ""))
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_start(18)
        .margin_end(18)
        .margin_top(18)
        .margin_bottom(24)
        .build();
    let intro = gtk::Label::new(Some(
        "Use these shortcuts while presenting, editing, or rehearsing. Press ? at any time to open this help.",
    ));
    intro.set_wrap(true);
    intro.set_halign(gtk::Align::Start);
    intro.add_css_class("dimmed");
    content.append(&intro);

    add_shortcut_group(
        &content,
        "Keyboard Shortcuts",
        &[
            ("Open a presentation", "<Primary>o"),
            ("Create a presentation", "<Primary>n"),
            ("Present the selected presentation", "<Primary>p"),
            ("Rehearse the selected presentation", "<Primary><Shift>r"),
            ("Export the selected presentation", "<Primary>e"),
            ("Editor: save", "<Primary>s"),
            ("Editor: save as", "<Primary><Shift>s"),
            ("Editor: import an asset", "<Primary>i"),
            ("Editor: complete a setting or asset", "<Primary>space"),
            ("Editor: present current source", "<Primary>Return"),
            (
                "Presentation: next slide (also Down, Space or Page Down)",
                "Right",
            ),
            (
                "Presentation: previous slide (also Up, Backspace or Page Up)",
                "Left",
            ),
            ("Presentation: first slide", "Home"),
            ("Presentation: last slide", "End"),
            ("Presentation: toggle fullscreen", "F11"),
            ("Presentation: blank the audience screen", "B"),
            ("Presentation: retry camera access", "C"),
            ("Presentation: run the slide command", "Return"),
            ("Presentation: close", "Escape"),
            ("Speaker mode: show or hide the speaker window", "F1"),
            ("Speaker view: swap displays", "S"),
            ("Show keyboard shortcuts", "question"),
        ],
    );

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();
    toolbar.set_content(Some(&scrolled));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(window));
}

fn add_shortcut_group(content: &gtk::Box, title: &str, shortcuts: &[(&str, &str)]) {
    let group = adw::PreferencesGroup::builder().title(title).build();
    for (title, primary) in shortcuts {
        let row = adw::ActionRow::builder().title(*title).build();
        let label = adw::ShortcutLabel::new(primary);
        label.set_halign(gtk::Align::End);
        label.set_valign(gtk::Align::Center);
        row.add_suffix(&label);
        group.add(&row);
    }
    content.append(&group);
}

fn install_shortcut_access(window: &adw::ApplicationWindow) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        window,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, modifiers| {
            let question = key == gdk::Key::question
                || (key == gdk::Key::slash
                    && modifiers.contains(gdk::ModifierType::CONTROL_MASK)
                    && modifiers.contains(gdk::ModifierType::SHIFT_MASK));
            if question {
                show_shortcuts(&window);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    ));
    window.add_controller(keys);
}
