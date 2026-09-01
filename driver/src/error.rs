// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use masterror::prelude::*;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceErrorKind {
    NotFound,
    ConnectionFailed,
    InvalidResponse,
    UnsupportedFeature,
    CommandFailed,
    Timeout
}

/// Converts a [`DeviceErrorKind`] into a [`masterror::AppError`].
///
/// Maps each logical device failure to the closest HTTP-style error category
/// so callers can branch on `AppError` uniformly.
///
/// # Examples
///
/// ```
/// use logi_mx_driver::error::{DeviceErrorKind, Result};
///
/// let result: Result<()> = Err(DeviceErrorKind::NotFound.into());
/// assert!(result.is_err());
/// ```
impl From<DeviceErrorKind> for AppError {
    fn from(kind: DeviceErrorKind) -> Self {
        match kind {
            DeviceErrorKind::NotFound => Self::not_found("Device not found or not connected"),
            DeviceErrorKind::ConnectionFailed => {
                Self::internal("Failed to establish device connection")
            }
            DeviceErrorKind::InvalidResponse => {
                Self::internal("Received invalid response from device")
            }
            DeviceErrorKind::UnsupportedFeature => {
                Self::bad_request("Feature not supported by this device")
            }
            DeviceErrorKind::CommandFailed => Self::internal("Device command execution failed"),
            DeviceErrorKind::Timeout => Self::timeout("Device communication timeout")
        }
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod error_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_error_kinds_convert() {
        let kinds = vec![
            DeviceErrorKind::NotFound,
            DeviceErrorKind::ConnectionFailed,
            DeviceErrorKind::InvalidResponse,
            DeviceErrorKind::UnsupportedFeature,
            DeviceErrorKind::CommandFailed,
            DeviceErrorKind::Timeout,
        ];

        for kind in kinds {
            let error: AppError = kind.into();
            assert!(!error.to_string().is_empty());
        }
    }

    #[test]
    fn test_result_type_ok() {
        let result: Result<i32> = Ok(42);
        assert!(result.is_ok());
        assert_eq!(result.as_ref().unwrap(), &42);
    }

    #[test]
    fn test_result_type_err() {
        let result: Result<i32> = Err(DeviceErrorKind::NotFound.into());
        assert!(result.is_err());
    }
}
