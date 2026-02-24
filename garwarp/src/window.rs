use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentWindowContext {
    X11 { window_id: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParentWindowError {
    Empty,
    InvalidFormat(String),
    UnsupportedBackend(String),
    InvalidWindowId(String),
}

impl fmt::Display for ParentWindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "parent window is empty"),
            Self::InvalidFormat(value) => write!(f, "invalid parent-window format: {value}"),
            Self::UnsupportedBackend(value) => {
                write!(f, "unsupported parent-window backend: {value}")
            }
            Self::InvalidWindowId(value) => write!(f, "invalid x11 window id: {value}"),
        }
    }
}

impl std::error::Error for ParentWindowError {}

pub fn parse_parent_window(input: &str) -> Result<ParentWindowContext, ParentWindowError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(ParentWindowError::Empty);
    }

    let (backend, value) = trimmed
        .split_once(':')
        .ok_or_else(|| ParentWindowError::InvalidFormat(trimmed.to_string()))?;

    if backend != "x11" {
        return Err(ParentWindowError::UnsupportedBackend(backend.to_string()));
    }

    let window_id = parse_x11_window_id(value)?;
    Ok(ParentWindowContext::X11 { window_id })
}

pub fn parse_optional_parent_window(
    input: Option<&str>,
) -> Result<Option<ParentWindowContext>, ParentWindowError> {
    match input {
        Some(value) if !value.trim().is_empty() => parse_parent_window(value).map(Some),
        _ => Ok(None),
    }
}

fn parse_x11_window_id(input: &str) -> Result<u64, ParentWindowError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(ParentWindowError::InvalidWindowId(input.to_string()));
    }

    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        return u64::from_str_radix(hex, 16)
            .map_err(|_| ParentWindowError::InvalidWindowId(input.to_string()));
    }

    trimmed
        .parse::<u64>()
        .map_err(|_| ParentWindowError::InvalidWindowId(input.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{ParentWindowContext, parse_optional_parent_window, parse_parent_window};

    #[test]
    fn parse_x11_hex_parent_window() {
        let parsed = parse_parent_window("x11:0x2a").expect("hex x11 window should parse");
        assert_eq!(parsed, ParentWindowContext::X11 { window_id: 42 });
    }

    #[test]
    fn parse_x11_decimal_parent_window() {
        let parsed = parse_parent_window("x11:42").expect("decimal x11 window should parse");
        assert_eq!(parsed, ParentWindowContext::X11 { window_id: 42 });
    }

    #[test]
    fn reject_unknown_backend() {
        let parsed = parse_parent_window("wayland:abc");
        assert!(parsed.is_err());
    }

    #[test]
    fn empty_optional_parent_window_is_none() {
        let parsed = parse_optional_parent_window(Some("  ")).expect("empty value should parse");
        assert!(parsed.is_none());
    }
}
