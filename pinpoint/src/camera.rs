use gst::prelude::*;
use gtk::gio::prelude::*;
use gtk::glib::variant::{FromVariant, Handle, ObjectPath, ToVariant};
use gtk::{gdk, gio, glib};
use std::cell::RefCell;
use std::collections::HashMap;
use std::os::fd::{AsRawFd, OwnedFd};
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};

const PORTAL_BUS_NAME: &str = "org.freedesktop.portal.Desktop";
const PORTAL_OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";
const CAMERA_INTERFACE: &str = "org.freedesktop.portal.Camera";
const REQUEST_INTERFACE: &str = "org.freedesktop.portal.Request";

pub struct CameraMedia {
    pipeline: gst::Pipeline,
    paintable: gdk::Paintable,
    bus: gst::Bus,
    error: Arc<Mutex<Option<String>>>,
    _remote_fd: OwnedFd,
}

impl CameraMedia {
    fn new(remote_fd: OwnedFd) -> Result<Self, String> {
        let source = gst::ElementFactory::make("pipewiresrc")
            .build()
            .map_err(|error| error.to_string())?;
        let sink = gst::ElementFactory::make("gtk4paintablesink")
            .build()
            .map_err(|error| error.to_string())?;
        source.set_property("fd", remote_fd.as_raw_fd());
        let paintable = sink.property::<gdk::Paintable>("paintable");
        let pipeline = gst::Pipeline::new();
        pipeline
            .add_many([&source, &sink])
            .map_err(|error| error.to_string())?;
        source.link(&sink).map_err(|error| error.to_string())?;
        let bus = pipeline
            .bus()
            .ok_or_else(|| "GStreamer camera pipeline has no message bus".to_owned())?;
        let error = Arc::new(Mutex::new(None));
        bus.add_signal_watch();
        bus.connect_message(
            Some("error"),
            glib::clone!(
                #[strong]
                error,
                move |_, message| {
                    if let gst::MessageView::Error(message) = message.view() {
                        *error.lock().expect("camera error lock") =
                            Some(message.error().to_string());
                    }
                }
            ),
        );
        Ok(Self {
            pipeline,
            paintable,
            bus,
            error,
            _remote_fd: remote_fd,
        })
    }

    pub fn paintable(&self) -> &gdk::Paintable {
        &self.paintable
    }

    pub fn set_playing(&self, playing: bool) -> Result<(), String> {
        self.pipeline
            .set_state(if playing {
                gst::State::Playing
            } else {
                gst::State::Paused
            })
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().expect("camera error lock").clone()
    }
}

impl Drop for CameraMedia {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
        self.bus.remove_signal_watch();
    }
}

pub enum CameraPortalEvent {
    Ready(CameraMedia),
    Denied,
    Error(String),
}

struct RequestShared {
    cancellable: gio::Cancellable,
    connection: RefCell<Option<gio::DBusConnection>>,
    request_path: RefCell<Option<String>>,
    subscription: RefCell<Option<gio::SignalSubscription>>,
    callback: Box<dyn Fn(CameraPortalEvent)>,
}

impl RequestShared {
    fn finish(&self, event: CameraPortalEvent) {
        self.subscription.borrow_mut().take();
        self.request_path.borrow_mut().take();
        (self.callback)(event);
    }
}

pub struct CameraRequest {
    shared: Rc<RequestShared>,
}

impl CameraRequest {
    pub fn start(callback: impl Fn(CameraPortalEvent) + 'static) -> Self {
        let shared = Rc::new(RequestShared {
            cancellable: gio::Cancellable::new(),
            connection: RefCell::new(None),
            request_path: RefCell::new(None),
            subscription: RefCell::new(None),
            callback: Box::new(callback),
        });
        let weak = Rc::downgrade(&shared);
        gio::bus_get(
            gio::BusType::Session,
            Some(&shared.cancellable),
            move |result| {
                let Some(shared) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(connection) => begin_access_request(&shared, connection),
                    Err(error) => shared.finish(CameraPortalEvent::Error(format!(
                        "Unable to connect to the camera portal: {error}"
                    ))),
                }
            },
        );
        Self { shared }
    }
}

impl Drop for CameraRequest {
    fn drop(&mut self) {
        self.shared.cancellable.cancel();
        self.shared.subscription.borrow_mut().take();
        let connection = self.shared.connection.borrow().clone();
        let request_path = self.shared.request_path.borrow_mut().take();
        if let (Some(connection), Some(request_path)) = (connection, request_path) {
            connection.call(
                Some(PORTAL_BUS_NAME),
                &request_path,
                REQUEST_INTERFACE,
                "Close",
                Some(&().to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
                |_| {},
            );
        }
    }
}

fn begin_access_request(shared: &Rc<RequestShared>, connection: gio::DBusConnection) {
    *shared.connection.borrow_mut() = Some(connection.clone());
    let sender = connection
        .unique_name()
        .map(|name| name.trim_start_matches(':').to_owned())
        .unwrap_or_else(|| "pinpoint".to_owned())
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let token = format!("pinpoint_{}_{}", std::process::id(), next_token());
    let request_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    *shared.request_path.borrow_mut() = Some(request_path.clone());

    let weak: Weak<RequestShared> = Rc::downgrade(shared);
    let subscription = connection.subscribe_to_signal(
        Some(PORTAL_BUS_NAME),
        Some(REQUEST_INTERFACE),
        Some("Response"),
        Some(&request_path),
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            if let Some(shared) = weak.upgrade() {
                handle_response(&shared, signal.connection, signal.parameters);
            }
        },
    );
    *shared.subscription.borrow_mut() = Some(subscription);

    let options = glib::VariantDict::new(None);
    options.insert("handle_token", token);
    let parameters = (options.end(),).to_variant();
    let weak = Rc::downgrade(shared);
    connection.call(
        Some(PORTAL_BUS_NAME),
        PORTAL_OBJECT_PATH,
        CAMERA_INTERFACE,
        "AccessCamera",
        Some(&parameters),
        glib::VariantTy::new("(o)").ok(),
        gio::DBusCallFlags::NONE,
        -1,
        Some(&shared.cancellable),
        move |result| {
            let Some(shared) = weak.upgrade() else {
                return;
            };
            match result {
                Ok(reply) => {
                    let returned =
                        <(ObjectPath,)>::from_variant(&reply).map(|(path,)| path.to_string());
                    if returned.as_deref() != shared.request_path.borrow().as_deref() {
                        eprintln!("PINPOINT CAMERA portal returned an unexpected request path");
                    }
                }
                Err(error) => shared.finish(CameraPortalEvent::Error(format!(
                    "Unable to request camera access: {error}"
                ))),
            }
        },
    );
}

fn handle_response(
    shared: &Rc<RequestShared>,
    connection: &gio::DBusConnection,
    parameters: &glib::Variant,
) {
    shared.subscription.borrow_mut().take();
    let Some((response, _results)) =
        <(u32, HashMap<String, glib::Variant>)>::from_variant(parameters)
    else {
        shared.finish(CameraPortalEvent::Error(
            "Camera portal returned an invalid response".to_owned(),
        ));
        return;
    };
    if response != 0 {
        shared.finish(if response == 1 {
            CameraPortalEvent::Denied
        } else {
            CameraPortalEvent::Error(format!(
                "Camera portal request failed with response {response}"
            ))
        });
        return;
    }

    let parameters = (glib::VariantDict::new(None).end(),).to_variant();
    let weak = Rc::downgrade(shared);
    connection.call_with_unix_fd_list(
        Some(PORTAL_BUS_NAME),
        PORTAL_OBJECT_PATH,
        CAMERA_INTERFACE,
        "OpenPipeWireRemote",
        Some(&parameters),
        glib::VariantTy::new("(h)").ok(),
        gio::DBusCallFlags::NONE,
        -1,
        gio::UnixFDList::NONE,
        Some(&shared.cancellable),
        move |result| {
            let Some(shared) = weak.upgrade() else {
                return;
            };
            let event = result
                .map_err(|error| format!("Unable to open the camera PipeWire remote: {error}"))
                .and_then(|(reply, fd_list)| {
                    let (Handle(index),) = <(Handle,)>::from_variant(&reply)
                        .ok_or_else(|| "Camera portal returned an invalid descriptor".to_owned())?;
                    let fd_list = fd_list
                        .ok_or_else(|| "Camera portal returned no descriptor list".to_owned())?;
                    let fd = fd_list.get(index).map_err(|error| {
                        format!("Unable to receive the camera PipeWire descriptor: {error}")
                    })?;
                    CameraMedia::new(fd)
                })
                .map(CameraPortalEvent::Ready)
                .unwrap_or_else(CameraPortalEvent::Error);
            shared.finish(event);
        },
    );
}

fn next_token() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static TOKEN: AtomicU64 = AtomicU64::new(1);
    TOKEN.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_response_shape_parses() {
        let response = (0u32, HashMap::<String, glib::Variant>::new()).to_variant();
        let parsed = <(u32, HashMap<String, glib::Variant>)>::from_variant(&response);
        assert_eq!(parsed.map(|(code, _)| code), Some(0));
    }

    #[test]
    fn request_tokens_are_distinct() {
        assert_ne!(next_token(), next_token());
    }
}
