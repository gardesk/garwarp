use zbus::{blocking::Connection, interface};

pub const BACKEND_DBUS_NAME: &str = "org.freedesktop.impl.portal.desktop.garwarp";
pub const BACKEND_OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";
const INTERFACE_VERSION: u32 = 1;

pub struct SessionNameGuard {
    _connection: Connection,
}

impl SessionNameGuard {
    pub fn acquire() -> zbus::Result<Self> {
        let connection = Connection::session()?;
        connection.request_name(BACKEND_DBUS_NAME)?;
        {
            let object_server = connection.object_server();
            object_server.at(BACKEND_OBJECT_PATH, ScreenshotPortal)?;
            object_server.at(BACKEND_OBJECT_PATH, FileChooserPortal)?;
            object_server.at(BACKEND_OBJECT_PATH, AppChooserPortal)?;
        }
        Ok(Self {
            _connection: connection,
        })
    }
}

#[derive(Debug)]
struct ScreenshotPortal;

#[interface(name = "org.freedesktop.impl.portal.Screenshot")]
impl ScreenshotPortal {
    #[zbus(property)]
    fn version(&self) -> u32 {
        INTERFACE_VERSION
    }
}

#[derive(Debug)]
struct FileChooserPortal;

#[interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooserPortal {
    #[zbus(property)]
    fn version(&self) -> u32 {
        INTERFACE_VERSION
    }
}

#[derive(Debug)]
struct AppChooserPortal;

#[interface(name = "org.freedesktop.impl.portal.AppChooser")]
impl AppChooserPortal {
    #[zbus(property)]
    fn version(&self) -> u32 {
        INTERFACE_VERSION
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppChooserPortal, BACKEND_OBJECT_PATH, FileChooserPortal, INTERFACE_VERSION,
        ScreenshotPortal,
    };

    #[test]
    fn backend_object_path_is_portal_desktop_path() {
        assert_eq!(BACKEND_OBJECT_PATH, "/org/freedesktop/portal/desktop");
    }

    #[test]
    fn portal_interfaces_report_expected_version() {
        assert_eq!(ScreenshotPortal.version(), INTERFACE_VERSION);
        assert_eq!(FileChooserPortal.version(), INTERFACE_VERSION);
        assert_eq!(AppChooserPortal.version(), INTERFACE_VERSION);
    }
}
