use zbus::blocking::Connection;

pub const BACKEND_DBUS_NAME: &str = "org.freedesktop.impl.portal.desktop.garwarp";

pub struct SessionNameGuard {
    _connection: Connection,
}

impl SessionNameGuard {
    pub fn acquire() -> zbus::Result<Self> {
        let connection = Connection::session()?;
        connection.request_name(BACKEND_DBUS_NAME)?;
        Ok(Self {
            _connection: connection,
        })
    }
}
