use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use gtk::glib::variant::ToVariant;
use std::cell::RefCell;
use std::rc::Rc;

const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

const INTROSPECTION: &str = "<node>
 <interface name='org.mpris.MediaPlayer2'>
  <method name='Raise'/><method name='Quit'/>
  <property name='CanQuit' type='b' access='read'/><property name='CanRaise' type='b' access='read'/><property name='HasTrackList' type='b' access='read'/>
  <property name='Identity' type='s' access='read'/><property name='DesktopEntry' type='s' access='read'/>
  <property name='SupportedUriSchemes' type='as' access='read'/><property name='SupportedMimeTypes' type='as' access='read'/>
  <property name='Fullscreen' type='b' access='readwrite'/><property name='CanSetFullscreen' type='b' access='read'/>
 </interface>
 <interface name='org.mpris.MediaPlayer2.Player'>
  <method name='Next'/><method name='Previous'/><method name='Pause'/><method name='PlayPause'/><method name='Stop'/><method name='Play'/>
  <method name='Seek'><arg direction='in' type='x' name='Offset'/></method>
  <method name='SetPosition'><arg direction='in' type='o' name='TrackId'/><arg direction='in' type='x' name='Position'/></method>
  <method name='OpenUri'><arg direction='in' type='s' name='Uri'/></method>
  <signal name='Seeked'><arg type='x' name='Position'/></signal>
  <property name='PlaybackStatus' type='s' access='read'/><property name='LoopStatus' type='s' access='readwrite'/><property name='Rate' type='d' access='readwrite'/><property name='Shuffle' type='b' access='readwrite'/>
  <property name='Metadata' type='a{sv}' access='read'/><property name='Volume' type='d' access='readwrite'/><property name='Position' type='x' access='read'/><property name='MinimumRate' type='d' access='read'/><property name='MaximumRate' type='d' access='read'/>
  <property name='CanGoNext' type='b' access='read'/><property name='CanGoPrevious' type='b' access='read'/><property name='CanPlay' type='b' access='read'/><property name='CanPause' type='b' access='read'/><property name='CanSeek' type='b' access='read'/><property name='CanControl' type='b' access='read'/>
 </interface>
</node>";

#[derive(Clone, Copy)]
pub enum Command {
    Next,
    Previous,
    Fullscreen(bool),
}

#[derive(Clone)]
pub struct State {
    pub presenting: bool,
    pub slide: usize,
    pub slides: usize,
    pub fullscreen: bool,
}

impl State {
    fn can_next(&self) -> bool {
        self.presenting && self.slide + 1 < self.slides
    }

    fn can_previous(&self) -> bool {
        self.presenting && self.slide > 0
    }

    fn metadata(&self) -> glib::Variant {
        let dictionary = glib::VariantDict::new(None);
        if self.presenting && self.slides > 0 {
            dictionary.insert(
                "mpris:trackid",
                format!("/com/nedrichards/pinpoint/Slide/s{}", self.slide + 1),
            );
            dictionary.insert(
                "xesam:title",
                format!("Slide {} of {}", self.slide + 1, self.slides),
            );
            dictionary.insert("xesam:album", "Pinpoint presentation");
        }
        dictionary.end()
    }
}

pub struct Mpris {
    connection: gio::DBusConnection,
    bus_name: String,
    _registrations: Vec<gio::RegistrationId>,
    state: Rc<RefCell<State>>,
}

impl Mpris {
    pub fn new(
        app: &adw::Application,
        state: Rc<RefCell<State>>,
        dispatch: impl Fn(Command) + 'static,
    ) -> Result<Self, String> {
        let connection = app
            .dbus_connection()
            .ok_or_else(|| "application D-Bus connection is unavailable".to_owned())?;
        let unique_name = connection
            .unique_name()
            .ok_or_else(|| "D-Bus unique name is unavailable".to_owned())?;
        let instance = unique_name.trim_start_matches(':').replace(['.', '-'], "_");
        let bus_name = format!(
            "org.mpris.MediaPlayer2.{}.instance_{instance}",
            app.application_id().unwrap_or_default()
        );
        let reply = connection
            .call_sync(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "RequestName",
                Some(&(bus_name.as_str(), 4_u32).to_variant()),
                Some(glib::VariantTy::new("(u)").expect("valid variant type")),
                gio::DBusCallFlags::NONE,
                -1,
                None::<&gio::Cancellable>,
            )
            .map_err(|error| error.to_string())?;
        if reply.child_get::<u32>(0) != 1 {
            return Err(format!("MPRIS name {bus_name} is already owned"));
        }
        let node = gio::DBusNodeInfo::for_xml(INTROSPECTION).map_err(|error| error.to_string())?;
        let root = node
            .lookup_interface(ROOT)
            .ok_or_else(|| "missing MPRIS root interface".to_owned())?;
        let player = node
            .lookup_interface(PLAYER)
            .ok_or_else(|| "missing MPRIS player interface".to_owned())?;
        let dispatch = Rc::new(dispatch);
        let root_state = state.clone();
        let root_dispatch = dispatch.clone();
        let root_registration = connection
            .register_object(OBJECT_PATH, &root)
            .method_call(move |_, _, _, _, method, _, invocation| match method {
                "Raise" | "Quit" => invocation.return_value(None),
                _ => invocation.return_dbus_error(
                    "org.freedesktop.DBus.Error.NotSupported",
                    "method is not meaningful for a presentation",
                ),
            })
            .property({
                let state = root_state.clone();
                move |_, _, _, _, property| root_property(&state.borrow(), property)
            })
            .set_property(move |_, _, _, _, property, value| {
                if property == "Fullscreen"
                    && let Some(fullscreen) = value.get::<bool>()
                    && root_state.borrow().presenting
                {
                    root_dispatch(Command::Fullscreen(fullscreen));
                    return true;
                }
                false
            })
            .build()
            .map_err(|error| error.to_string())?;
        let player_state = state.clone();
        let player_method_state = player_state.clone();
        let player_dispatch = dispatch.clone();
        let player_registration = connection
            .register_object(OBJECT_PATH, &player)
            .method_call(move |_, _, _, _, method, _, invocation| match method {
                "Next" if player_method_state.borrow().can_next() => {
                    player_dispatch(Command::Next);
                    invocation.return_value(None);
                }
                "Previous" if player_method_state.borrow().can_previous() => {
                    player_dispatch(Command::Previous);
                    invocation.return_value(None);
                }
                "Next" | "Previous" => invocation.return_value(None),
                _ => invocation.return_dbus_error(
                    "org.freedesktop.DBus.Error.NotSupported",
                    "method is not meaningful for a presentation",
                ),
            })
            .property(move |_, _, _, _, property| player_property(&player_state.borrow(), property))
            .set_property(|_, _, _, _, _, _| false)
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            connection,
            bus_name,
            _registrations: vec![root_registration, player_registration],
            state,
        })
    }

    pub fn bus_name(&self) -> &str {
        &self.bus_name
    }

    pub fn sync(&self, state: State) {
        *self.state.borrow_mut() = state;
        let _ = self.connection.emit_signal(
            None,
            OBJECT_PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            Some(
                &(
                    PLAYER,
                    changed_player(&self.state.borrow()),
                    Vec::<String>::new(),
                )
                    .to_variant(),
            ),
        );
        let _ = self.connection.emit_signal(
            None,
            OBJECT_PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            Some(
                &(
                    ROOT,
                    changed_root(&self.state.borrow()),
                    Vec::<String>::new(),
                )
                    .to_variant(),
            ),
        );
    }
}

impl Drop for Mpris {
    fn drop(&mut self) {
        let _ = self.connection.call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "ReleaseName",
            Some(&(self.bus_name.as_str(),).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            None::<&gio::Cancellable>,
        );
    }
}

fn root_property(state: &State, property: &str) -> glib::Variant {
    match property {
        "Identity" => "Pinpoint".to_variant(),
        "DesktopEntry" => "com.nedrichards.pinpoint".to_variant(),
        "SupportedUriSchemes" | "SupportedMimeTypes" => Vec::<String>::new().to_variant(),
        "Fullscreen" => state.fullscreen.to_variant(),
        "CanSetFullscreen" => state.presenting.to_variant(),
        _ => false.to_variant(),
    }
}

fn player_property(state: &State, property: &str) -> glib::Variant {
    match property {
        "PlaybackStatus" => "Stopped".to_variant(),
        "LoopStatus" => "None".to_variant(),
        "Metadata" => state.metadata(),
        "CanGoNext" => state.can_next().to_variant(),
        "CanGoPrevious" => state.can_previous().to_variant(),
        "CanControl" => state.presenting.to_variant(),
        "Rate" | "Volume" | "MinimumRate" | "MaximumRate" => 1.0_f64.to_variant(),
        "Position" => 0_i64.to_variant(),
        _ => false.to_variant(),
    }
}

fn changed_player(state: &State) -> glib::Variant {
    let dictionary = glib::VariantDict::new(None);
    dictionary.insert_value("Metadata", &state.metadata());
    dictionary.insert("CanGoNext", state.can_next());
    dictionary.insert("CanGoPrevious", state.can_previous());
    dictionary.insert("CanControl", state.presenting);
    dictionary.end()
}

fn changed_root(state: &State) -> glib::Variant {
    let dictionary = glib::VariantDict::new(None);
    dictionary.insert("Fullscreen", state.fullscreen);
    dictionary.insert("CanSetFullscreen", state.presenting);
    dictionary.end()
}
