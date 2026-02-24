use std::collections::HashMap;

use zbus::{
    blocking::Connection,
    interface,
    message::Header,
    zvariant::{OwnedObjectPath, OwnedValue},
};

use crate::error::{PortalError, map_portal_error};
use crate::portal::derive_request_id_from_handle;

pub const BACKEND_DBUS_NAME: &str = "org.freedesktop.impl.portal.desktop.garwarp";
pub const BACKEND_OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";
const INTERFACE_VERSION: u32 = 1;
type PortalMethodReply = (u32, HashMap<String, OwnedValue>);

fn failed_response() -> PortalMethodReply {
    failure_response(&PortalError::InternalFailure)
}

fn failure_response(error: &PortalError) -> PortalMethodReply {
    let mapping = map_portal_error(error);
    (mapping.code as u32, HashMap::new())
}

fn request_id_for_call(
    request_handle: &OwnedObjectPath,
    header: Header<'_>,
) -> Result<String, PortalError> {
    let sender = header
        .sender()
        .map(|sender| sender.as_str())
        .ok_or(PortalError::UnauthorizedClient)?;

    derive_request_id_from_handle(sender, request_handle.as_str())
}

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
    fn screenshot(
        &self,
        handle: OwnedObjectPath,
        _app_id: &str,
        _parent_window: &str,
        _options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
    ) -> PortalMethodReply {
        if let Err(error) = request_id_for_call(&handle, header) {
            return failure_response(&error);
        }
        failed_response()
    }

    fn pick_color(
        &self,
        handle: OwnedObjectPath,
        _app_id: &str,
        _parent_window: &str,
        _options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
    ) -> PortalMethodReply {
        if let Err(error) = request_id_for_call(&handle, header) {
            return failure_response(&error);
        }
        failed_response()
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        INTERFACE_VERSION
    }
}

#[derive(Debug)]
struct FileChooserPortal;

#[interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooserPortal {
    fn open_file(
        &self,
        handle: OwnedObjectPath,
        _app_id: &str,
        _parent_window: &str,
        _title: &str,
        _options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
    ) -> PortalMethodReply {
        if let Err(error) = request_id_for_call(&handle, header) {
            return failure_response(&error);
        }
        failed_response()
    }

    fn save_file(
        &self,
        handle: OwnedObjectPath,
        _app_id: &str,
        _parent_window: &str,
        _title: &str,
        _options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
    ) -> PortalMethodReply {
        if let Err(error) = request_id_for_call(&handle, header) {
            return failure_response(&error);
        }
        failed_response()
    }

    fn save_files(
        &self,
        handle: OwnedObjectPath,
        _app_id: &str,
        _parent_window: &str,
        _title: &str,
        _options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
    ) -> PortalMethodReply {
        if let Err(error) = request_id_for_call(&handle, header) {
            return failure_response(&error);
        }
        failed_response()
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        INTERFACE_VERSION
    }
}

#[derive(Debug)]
struct AppChooserPortal;

#[interface(name = "org.freedesktop.impl.portal.AppChooser")]
impl AppChooserPortal {
    fn choose_application(
        &self,
        handle: OwnedObjectPath,
        _app_id: &str,
        _parent_window: &str,
        _choices: Vec<String>,
        _options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
    ) -> PortalMethodReply {
        if let Err(error) = request_id_for_call(&handle, header) {
            return failure_response(&error);
        }
        failed_response()
    }

    fn update_choices(
        &self,
        handle: OwnedObjectPath,
        _choices: Vec<String>,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<()> {
        request_id_for_call(&handle, header)
            .map(|_| ())
            .map_err(|error| zbus::fdo::Error::Failed(map_portal_error(&error).reason.to_string()))
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        INTERFACE_VERSION
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        AppChooserPortal, BACKEND_OBJECT_PATH, FileChooserPortal, INTERFACE_VERSION,
        ScreenshotPortal,
    };
    use crate::error::PortalResponseCode;
    use zbus::{
        message::{Header, Message},
        zvariant::{OwnedObjectPath, OwnedValue},
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

    #[test]
    fn screenshot_and_pick_color_return_failed_placeholder() {
        let options = HashMap::<String, OwnedValue>::new();
        let (response, results) = ScreenshotPortal.screenshot(
            request_handle_path(),
            "org.test.App",
            "x11:0x2a",
            options.clone(),
            test_call_header(),
        );
        assert_eq!(response, PortalResponseCode::Failed as u32);
        assert!(results.is_empty());

        let (response, results) = ScreenshotPortal.pick_color(
            request_handle_path(),
            "org.test.App",
            "",
            options,
            test_call_header(),
        );
        assert_eq!(response, PortalResponseCode::Failed as u32);
        assert!(results.is_empty());
    }

    #[test]
    fn file_chooser_methods_return_failed_placeholder() {
        let options = HashMap::<String, OwnedValue>::new();
        let (response, results) = FileChooserPortal.open_file(
            request_handle_path(),
            "org.test.App",
            "",
            "Open",
            options.clone(),
            test_call_header(),
        );
        assert_eq!(response, PortalResponseCode::Failed as u32);
        assert!(results.is_empty());

        let (response, results) = FileChooserPortal.save_file(
            request_handle_path(),
            "org.test.App",
            "",
            "Save",
            options.clone(),
            test_call_header(),
        );
        assert_eq!(response, PortalResponseCode::Failed as u32);
        assert!(results.is_empty());

        let (response, results) = FileChooserPortal.save_files(
            request_handle_path(),
            "org.test.App",
            "",
            "Save",
            options,
            test_call_header(),
        );
        assert_eq!(response, PortalResponseCode::Failed as u32);
        assert!(results.is_empty());
    }

    #[test]
    fn app_chooser_methods_have_stable_placeholders() {
        let options = HashMap::<String, OwnedValue>::new();
        let choices = vec!["org.test.Viewer".to_string()];
        let (response, results) = AppChooserPortal.choose_application(
            request_handle_path(),
            "org.test.App",
            "",
            choices.clone(),
            options,
            test_call_header(),
        );
        assert_eq!(response, PortalResponseCode::Failed as u32);
        assert!(results.is_empty());

        let error = AppChooserPortal
            .update_choices(request_handle_path(), choices, test_call_header())
            .expect_err("update choices should reject missing sender metadata");
        match error {
            zbus::fdo::Error::Failed(reason) => assert_eq!(reason, "unauthorized_client"),
            other => panic!("unexpected update choices error: {other:?}"),
        }
    }

    fn request_handle_path() -> OwnedObjectPath {
        OwnedObjectPath::try_from("/org/freedesktop/portal/desktop/request/1_42/token_1")
            .expect("valid request object path")
    }

    fn test_call_header() -> Header<'static> {
        let call = Message::method_call(BACKEND_OBJECT_PATH, "TestCall")
            .expect("test call builder")
            .interface("org.freedesktop.impl.portal.Screenshot")
            .expect("test interface")
            .build(&())
            .expect("test message");
        Box::leak(Box::new(call)).header()
    }
}
