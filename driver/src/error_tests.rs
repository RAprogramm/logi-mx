// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn test_device_error_not_found() {
        let error: AppError = DeviceErrorKind::NotFound.into();
        let msg = error.to_string();
        assert!(msg.contains("not found") || msg.contains("not connected"));
    }

    #[test]
    fn test_device_error_connection_failed() {
        let error: AppError = DeviceErrorKind::ConnectionFailed.into();
        let msg = error.to_string();
        assert!(msg.contains("connection") || msg.contains("establish"));
    }

    #[test]
    fn test_device_error_invalid_response() {
        let error: AppError = DeviceErrorKind::InvalidResponse.into();
        let msg = error.to_string();
        assert!(msg.contains("invalid") || msg.contains("response"));
    }

    #[test]
    fn test_device_error_unsupported_feature() {
        let error: AppError = DeviceErrorKind::UnsupportedFeature.into();
        let msg = error.to_string();
        assert!(msg.contains("not supported") || msg.contains("unsupported"));
    }

    #[test]
    fn test_device_error_command_failed() {
        let error: AppError = DeviceErrorKind::CommandFailed.into();
        let msg = error.to_string();
        assert!(msg.contains("failed") || msg.contains("execution"));
    }

    #[test]
    fn test_device_error_timeout() {
        let error: AppError = DeviceErrorKind::Timeout.into();
        let msg = error.to_string();
        assert!(msg.contains("timeout") || msg.contains("communication"));
    }

    #[test]
    fn test_error_kind_equality() {
        assert_eq!(DeviceErrorKind::NotFound, DeviceErrorKind::NotFound);
        assert_ne!(DeviceErrorKind::NotFound, DeviceErrorKind::Timeout);
    }

    #[test]
    fn test_error_kind_copy() {
        let kind1 = DeviceErrorKind::Timeout;
        let kind2 = kind1;
        assert_eq!(kind1, kind2);
    }

    #[test]
    fn test_error_kind_debug() {
        let kind = DeviceErrorKind::InvalidResponse;
        let debug = format!("{:?}", kind);
        assert!(debug.contains("InvalidResponse"));
    }

    #[test]
    fn test_all_error_kinds() {
        let kinds = vec![
            DeviceErrorKind::NotFound,
            DeviceErrorKind::ConnectionFailed,
            DeviceErrorKind::InvalidResponse,
            DeviceErrorKind::UnsupportedFeature,
            DeviceErrorKind::CommandFailed,
            DeviceErrorKind::Timeout,
        ];

        assert_eq!(kinds.len(), 6);
    }
}
