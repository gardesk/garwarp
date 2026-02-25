#![allow(dead_code)]

use std::collections::HashMap;

use zbus::zvariant::OwnedValue;

use crate::error::PortalError;

pub type PortalOptions = HashMap<String, OwnedValue>;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScreenshotOptions {
    pub modal: Option<bool>,
    pub interactive: Option<bool>,
    pub permission_store_checked: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileChooserOptions {
    pub accept_label: Option<String>,
    pub modal: Option<bool>,
    pub multiple: Option<bool>,
    pub directory: Option<bool>,
    pub current_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppChooserOptions {
    pub last_choice: Option<String>,
    pub modal: Option<bool>,
    pub content_type: Option<String>,
    pub uri: Option<String>,
    pub filename: Option<String>,
    pub activation_token: Option<String>,
}

pub fn parse_screenshot_options(options: &PortalOptions) -> Result<ScreenshotOptions, PortalError> {
    Ok(ScreenshotOptions {
        modal: bool_option(options, "modal")?,
        interactive: bool_option(options, "interactive")?,
        permission_store_checked: bool_option(options, "permission_store_checked")?,
    })
}

pub fn parse_file_chooser_options(
    options: &PortalOptions,
) -> Result<FileChooserOptions, PortalError> {
    Ok(FileChooserOptions {
        accept_label: string_option(options, "accept_label")?,
        modal: bool_option(options, "modal")?,
        multiple: bool_option(options, "multiple")?,
        directory: bool_option(options, "directory")?,
        current_name: string_option(options, "current_name")?,
    })
}

pub fn parse_app_chooser_options(
    options: &PortalOptions,
) -> Result<AppChooserOptions, PortalError> {
    Ok(AppChooserOptions {
        last_choice: string_option(options, "last_choice")?,
        modal: bool_option(options, "modal")?,
        content_type: string_option(options, "content_type")?,
        uri: string_option(options, "uri")?,
        filename: string_option(options, "filename")?,
        activation_token: string_option(options, "activation_token")?,
    })
}

fn bool_option(options: &PortalOptions, key: &str) -> Result<Option<bool>, PortalError> {
    options
        .get(key)
        .map(|value| bool::try_from(value).map_err(|_| PortalError::InvalidRequestPayload))
        .transpose()
}

fn string_option(options: &PortalOptions, key: &str) -> Result<Option<String>, PortalError> {
    options
        .get(key)
        .map(|value| {
            <&str>::try_from(value)
                .map(|value| value.to_string())
                .map_err(|_| PortalError::InvalidRequestPayload)
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::{
        AppChooserOptions, FileChooserOptions, PortalOptions, ScreenshotOptions,
        parse_app_chooser_options, parse_file_chooser_options, parse_screenshot_options,
    };
    use crate::error::PortalError;
    use zbus::zvariant::{OwnedValue, Str};

    #[test]
    fn parse_screenshot_options_accepts_supported_keys() {
        let mut options = PortalOptions::new();
        options.insert("modal".to_string(), OwnedValue::from(true));
        options.insert("interactive".to_string(), OwnedValue::from(false));
        options.insert(
            "permission_store_checked".to_string(),
            OwnedValue::from(true),
        );

        let parsed = parse_screenshot_options(&options).expect("screenshot options");
        assert_eq!(
            parsed,
            ScreenshotOptions {
                modal: Some(true),
                interactive: Some(false),
                permission_store_checked: Some(true),
            }
        );
    }

    #[test]
    fn parse_screenshot_options_rejects_wrong_type() {
        let mut options = PortalOptions::new();
        options.insert("modal".to_string(), str_value("yes"));

        let parsed = parse_screenshot_options(&options);
        assert_eq!(parsed, Err(PortalError::InvalidRequestPayload));
    }

    #[test]
    fn parse_file_chooser_options_accepts_supported_keys() {
        let mut options = PortalOptions::new();
        options.insert("accept_label".to_string(), str_value("Open"));
        options.insert("modal".to_string(), OwnedValue::from(true));
        options.insert("multiple".to_string(), OwnedValue::from(false));
        options.insert("directory".to_string(), OwnedValue::from(false));
        options.insert("current_name".to_string(), str_value("image.png"));

        let parsed = parse_file_chooser_options(&options).expect("file chooser options");
        assert_eq!(
            parsed,
            FileChooserOptions {
                accept_label: Some("Open".to_string()),
                modal: Some(true),
                multiple: Some(false),
                directory: Some(false),
                current_name: Some("image.png".to_string()),
            }
        );
    }

    #[test]
    fn parse_app_chooser_options_accepts_supported_keys() {
        let mut options = PortalOptions::new();
        options.insert("last_choice".to_string(), str_value("org.test.Viewer"));
        options.insert("modal".to_string(), OwnedValue::from(true));
        options.insert("content_type".to_string(), str_value("image/png"));
        options.insert("uri".to_string(), str_value("file:///tmp/a.png"));
        options.insert("filename".to_string(), str_value("a.png"));
        options.insert("activation_token".to_string(), str_value("token-1"));

        let parsed = parse_app_chooser_options(&options).expect("app chooser options");
        assert_eq!(
            parsed,
            AppChooserOptions {
                last_choice: Some("org.test.Viewer".to_string()),
                modal: Some(true),
                content_type: Some("image/png".to_string()),
                uri: Some("file:///tmp/a.png".to_string()),
                filename: Some("a.png".to_string()),
                activation_token: Some("token-1".to_string()),
            }
        );
    }

    #[test]
    fn parse_app_chooser_options_rejects_wrong_type() {
        let mut options = PortalOptions::new();
        options.insert("activation_token".to_string(), OwnedValue::from(99_u32));

        let parsed = parse_app_chooser_options(&options);
        assert_eq!(parsed, Err(PortalError::InvalidRequestPayload));
    }

    fn str_value(value: &str) -> OwnedValue {
        OwnedValue::from(Str::from(value))
    }
}
