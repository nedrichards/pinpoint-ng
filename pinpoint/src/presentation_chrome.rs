use crate::stage::Stage;
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

const CLOSE_FADE: Duration = Duration::from_millis(200);
const CLOSE_VISIBLE: Duration = Duration::from_millis(2_000);
const CURSOR_VISIBLE: Duration = Duration::from_millis(500);

#[derive(Clone)]
pub struct PresentationChrome {
    revealer: gtk::Revealer,
    hide_control: Rc<RefCell<Option<glib::SourceId>>>,
    notice: gtk::Revealer,
    notice_label: gtk::Label,
    notice_control: Rc<RefCell<Option<glib::SourceId>>>,
    problems: gtk::Revealer,
    problem_label: gtk::Label,
}

impl PresentationChrome {
    pub fn new(
        window: &adw::ApplicationWindow,
        stage: &Stage,
        child: &impl IsA<gtk::Widget>,
    ) -> (gtk::Overlay, Self) {
        window.set_decorated(false);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(child));

        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("circular");
        close.add_css_class("osd");
        close.set_tooltip_text(Some("End Presentation"));
        close.update_property(&[gtk::accessible::Property::Label("End Presentation")]);
        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .transition_duration(CLOSE_FADE.as_millis() as u32)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .margin_top(16)
            .margin_end(16)
            .child(&close)
            .build();
        overlay.add_overlay(&revealer);

        let notice_label = gtk::Label::new(None);
        let notice = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .transition_duration(CLOSE_FADE.as_millis() as u32)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .margin_bottom(24)
            .child(&notice_label)
            .build();
        notice_label.add_css_class("osd");
        notice_label.set_margin_start(14);
        notice_label.set_margin_end(14);
        notice_label.set_margin_top(8);
        notice_label.set_margin_bottom(8);
        overlay.add_overlay(&notice);

        let problem_label = gtk::Label::builder().xalign(0.0).wrap(true).build();
        let retry = gtk::Button::with_label("Retry");
        retry.set_tooltip_text(Some("Retry failed presentation assets"));
        let open_folder = gtk::Button::with_label("Open Folder");
        open_folder.set_tooltip_text(Some("Open the presentation folder"));
        let problem_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        problem_box.add_css_class("osd");
        problem_box.set_margin_start(14);
        problem_box.set_margin_end(14);
        problem_box.set_margin_top(8);
        problem_box.set_margin_bottom(8);
        problem_box.append(&problem_label);
        problem_box.append(&retry);
        problem_box.append(&open_folder);
        let problems = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .transition_duration(CLOSE_FADE.as_millis() as u32)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Start)
            .margin_top(24)
            .child(&problem_box)
            .build();
        overlay.add_overlay(&problems);

        let chrome = Self {
            revealer: revealer.clone(),
            hide_control: Rc::new(RefCell::new(None)),
            notice,
            notice_label,
            notice_control: Rc::new(RefCell::new(None)),
            problems,
            problem_label,
        };
        retry.connect_clicked(glib::clone!(
            #[weak]
            stage,
            move |_| stage.retry_failed_assets()
        ));
        open_folder.connect_clicked(glib::clone!(
            #[weak]
            stage,
            move |_| {
                let Some(folder) = stage.presentation_folder() else {
                    return;
                };
                if let Err(error) = gio::AppInfo::launch_default_for_uri(
                    &gio::File::for_path(folder).uri(),
                    None::<&gio::AppLaunchContext>,
                ) {
                    eprintln!("pinpoint: unable to open presentation folder: {error}");
                }
            }
        ));
        let weak_stage = stage.downgrade();
        let weak_problems = chrome.problems.downgrade();
        let weak_label = chrome.problem_label.downgrade();
        glib::timeout_add_local(Duration::from_millis(250), move || {
            let (Some(stage), Some(problems), Some(label)) = (
                weak_stage.upgrade(),
                weak_problems.upgrade(),
                weak_label.upgrade(),
            ) else {
                return glib::ControlFlow::Break;
            };
            if let Some(summary) = stage.asset_problem_summary() {
                label.set_label(&summary.message);
                label.set_tooltip_text(Some(&summary.details));
                problems.set_reveal_child(true);
            } else {
                problems.set_reveal_child(false);
                label.set_tooltip_text(None);
            }
            glib::ControlFlow::Continue
        });
        let hide_control = chrome.hide_control.clone();
        close.connect_clicked(glib::clone!(
            #[weak]
            window,
            #[weak]
            revealer,
            move |_| {
                if let Some(source) = hide_control.borrow_mut().take() {
                    source.remove();
                }
                revealer.set_reveal_child(false);
                window.close();
            }
        ));

        let cursor_timeout = Rc::new(RefCell::new(None::<glib::SourceId>));
        let pointer_position = Rc::new(Cell::new(None::<(f64, f64)>));
        let motion = gtk::EventControllerMotion::new();
        motion.set_propagation_phase(gtk::PropagationPhase::Capture);
        motion.connect_motion(glib::clone!(
            #[weak]
            stage,
            #[strong]
            chrome,
            #[strong]
            cursor_timeout,
            #[strong]
            pointer_position,
            move |_, x, y| {
                if pointer_position.get() == Some((x, y)) {
                    return;
                }
                pointer_position.set(Some((x, y)));
                if let Some(source) = cursor_timeout.borrow_mut().take() {
                    source.remove();
                }
                stage.set_cursor_from_name(Some("default"));
                chrome.reveal_for_input();
                let timeout_slot = cursor_timeout.clone();
                let source = glib::timeout_add_local_once(
                    CURSOR_VISIBLE,
                    glib::clone!(
                        #[weak]
                        stage,
                        move || {
                            timeout_slot.borrow_mut().take();
                            stage.set_cursor_from_name(Some("none"));
                        }
                    ),
                );
                *cursor_timeout.borrow_mut() = Some(source);
            }
        ));
        motion.connect_leave(glib::clone!(
            #[weak]
            stage,
            #[strong]
            cursor_timeout,
            move |_| {
                if let Some(source) = cursor_timeout.borrow_mut().take() {
                    source.remove();
                }
                stage.set_cursor_from_name(Some("default"));
            }
        ));
        overlay.add_controller(motion);
        (overlay, chrome)
    }

    pub fn reveal_for_input(&self) {
        self.revealer.set_reveal_child(true);
        if let Some(source) = self.hide_control.borrow_mut().take() {
            source.remove();
        }
        let revealer = self.revealer.clone();
        let timeout_slot = self.hide_control.clone();
        let source = glib::timeout_add_local_once(CLOSE_VISIBLE, move || {
            timeout_slot.borrow_mut().take();
            revealer.set_reveal_child(false);
        });
        *self.hide_control.borrow_mut() = Some(source);
    }

    pub fn hide(&self) {
        if let Some(source) = self.hide_control.borrow_mut().take() {
            source.remove();
        }
        self.revealer.set_reveal_child(false);
    }

    pub fn show_notice(&self, message: &str) {
        if let Some(source) = self.notice_control.borrow_mut().take() {
            source.remove();
        }
        self.notice_label.set_label(message);
        self.notice.set_reveal_child(true);
        let notice = self.notice.clone();
        let timeout_slot = self.notice_control.clone();
        let source = glib::timeout_add_local_once(CLOSE_VISIBLE, move || {
            timeout_slot.borrow_mut().take();
            notice.set_reveal_child(false);
        });
        *self.notice_control.borrow_mut() = Some(source);
    }

    pub fn is_revealed(&self) -> bool {
        self.revealer.reveals_child()
    }
}
