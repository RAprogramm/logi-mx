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

impl From<DeviceErrorKind> for AppError {
    fn from(kind: DeviceErrorKind) -> Self {
        match kind {
            DeviceErrorKind::NotFound => AppError::not_found("Device not found or not connected"),
            DeviceErrorKind::ConnectionFailed => {
                AppError::internal("Failed to establish device connection")
            }
            DeviceErrorKind::InvalidResponse => {
                AppError::internal("Received invalid response from device")
            }
            DeviceErrorKind::UnsupportedFeature => {
                AppError::bad_request("Feature not supported by this device")
            }
            DeviceErrorKind::CommandFailed => {
                AppError::internal("Device command execution failed")
            }
            DeviceErrorKind::Timeout => AppError::timeout("Device communication timeout")
        }
    }
}

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
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_result_type_err() {
        let result: Result<i32> = Err(DeviceErrorKind::NotFound.into());
        assert!(result.is_err());
    }
}
