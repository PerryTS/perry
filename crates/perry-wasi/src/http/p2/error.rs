// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use super::bindings::http::types::{self, ErrorCode};
use crate::http::{Error, FieldMapError};
use std::fmt;
use wasmtime::component::ResourceTableError;

/// A [`Result`] type where the error type defaults to [`HttpError`].
pub type HttpResult<T, E = HttpError> = Result<T, E>;

/// A `wasi:http`-specific error type used to represent either a trap or an
/// [`ErrorCode`].
///
/// Modeled after [`TrappableError`](wasmtime_wasi::TrappableError).
#[repr(transparent)]
pub struct HttpError {
    err: wasmtime::Error,
}

impl HttpError {
    /// Create a new `HttpError` that represents a trap.
    pub fn trap(err: impl Into<wasmtime::Error>) -> HttpError {
        HttpError { err: err.into() }
    }

    /// Downcast this error to an [`ErrorCode`].
    pub fn downcast(self) -> wasmtime::Result<ErrorCode> {
        self.err.downcast()
    }

    /// Downcast this error to a reference to an [`ErrorCode`]
    pub fn downcast_ref(&self) -> Option<&ErrorCode> {
        self.err.downcast_ref()
    }
}

impl From<ErrorCode> for HttpError {
    fn from(error: ErrorCode) -> Self {
        Self { err: error.into() }
    }
}

impl From<ResourceTableError> for HttpError {
    fn from(error: ResourceTableError) -> Self {
        HttpError::trap(error)
    }
}

impl fmt::Debug for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.err.fmt(f)
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.err.fmt(f)
    }
}

impl std::error::Error for HttpError {}

/// A [`Result`] type where the error type defaults to [`HeaderError`].
pub type HeaderResult<T, E = HeaderError> = Result<T, E>;

/// A `wasi:http`-specific error type used to represent either a trap or an
/// [`types::HeaderError`].
///
/// Modeled after [`TrappableError`](wasmtime_wasi::TrappableError).
#[repr(transparent)]
pub struct HeaderError {
    err: wasmtime::Error,
}

impl HeaderError {
    /// Create a new `HeaderError` that represents a trap.
    pub fn trap(err: impl Into<wasmtime::Error>) -> HeaderError {
        HeaderError { err: err.into() }
    }

    /// Downcast this error to an [`ErrorCode`].
    pub fn downcast(self) -> wasmtime::Result<types::HeaderError> {
        self.err.downcast()
    }

    /// Downcast this error to a reference to an [`ErrorCode`]
    pub fn downcast_ref(&self) -> Option<&types::HeaderError> {
        self.err.downcast_ref()
    }
}

impl From<types::HeaderError> for HeaderError {
    fn from(error: types::HeaderError) -> Self {
        Self { err: error.into() }
    }
}

impl From<ResourceTableError> for HeaderError {
    fn from(error: ResourceTableError) -> Self {
        HeaderError::trap(error)
    }
}

impl From<http::header::InvalidHeaderName> for HeaderError {
    fn from(_: http::header::InvalidHeaderName) -> Self {
        HeaderError::from(types::HeaderError::InvalidSyntax)
    }
}

impl From<http::header::InvalidHeaderValue> for HeaderError {
    fn from(_: http::header::InvalidHeaderValue) -> Self {
        HeaderError::from(types::HeaderError::InvalidSyntax)
    }
}

impl From<FieldMapError> for HeaderError {
    fn from(err: FieldMapError) -> Self {
        match err {
            FieldMapError::Immutable => types::HeaderError::Immutable.into(),
            FieldMapError::InvalidHeaderName | FieldMapError::InvalidHeaderValue => {
                types::HeaderError::InvalidSyntax.into()
            }
            FieldMapError::TooManyFields | FieldMapError::TotalSizeTooBig => HeaderError::trap(err),
            FieldMapError::Forbidden => types::HeaderError::Forbidden.into(),
        }
    }
}

impl fmt::Debug for HeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.err.fmt(f)
    }
}

impl fmt::Display for HeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.err.fmt(f)
    }
}

impl From<ErrorCode> for Error {
    fn from(e: ErrorCode) -> Self {
        match e {
            ErrorCode::DnsTimeout => Self::DnsTimeout,
            ErrorCode::DnsError(payload) => Self::DnsError {
                rcode: payload.rcode,
                info_code: payload.info_code,
            },
            ErrorCode::DestinationNotFound => Self::DestinationNotFound,
            ErrorCode::DestinationUnavailable => Self::DestinationUnavailable,
            ErrorCode::DestinationIpProhibited => Self::DestinationIpProhibited,
            ErrorCode::DestinationIpUnroutable => Self::DestinationIpUnroutable,
            ErrorCode::ConnectionRefused => Self::ConnectionRefused,
            ErrorCode::ConnectionTerminated => Self::ConnectionTerminated,
            ErrorCode::ConnectionTimeout => Self::ConnectionTimeout,
            ErrorCode::ConnectionReadTimeout => Self::ConnectionReadTimeout,
            ErrorCode::ConnectionWriteTimeout => Self::ConnectionWriteTimeout,
            ErrorCode::ConnectionLimitReached => Self::ConnectionLimitReached,
            ErrorCode::TlsProtocolError => Self::TlsProtocolError,
            ErrorCode::TlsCertificateError => Self::TlsCertificateError,
            ErrorCode::TlsAlertReceived(payload) => Self::TlsAlertReceived {
                alert_id: payload.alert_id,
                alert_message: payload.alert_message,
            },
            ErrorCode::HttpRequestDenied => Self::HttpRequestDenied,
            ErrorCode::HttpRequestLengthRequired => Self::HttpRequestLengthRequired,
            ErrorCode::HttpRequestBodySize(payload) => Self::HttpRequestBodySize(payload),
            ErrorCode::HttpRequestMethodInvalid => Self::HttpRequestMethodInvalid,
            ErrorCode::HttpRequestUriInvalid => Self::HttpRequestUriInvalid,
            ErrorCode::HttpRequestUriTooLong => Self::HttpRequestUriTooLong,
            ErrorCode::HttpRequestHeaderSectionSize(payload) => {
                Self::HttpRequestHeaderSectionSize(payload)
            }
            ErrorCode::HttpRequestHeaderSize(payload) => {
                let (field_name, field_size) = match payload {
                    Some(p) => (p.field_name, p.field_size),
                    None => (None, None),
                };
                Self::HttpRequestHeaderSize {
                    field_name,
                    field_size,
                }
            }
            ErrorCode::HttpRequestTrailerSectionSize(payload) => {
                Self::HttpRequestTrailerSectionSize(payload)
            }
            ErrorCode::HttpRequestTrailerSize(payload) => Self::HttpRequestTrailerSize {
                field_name: payload.field_name,
                field_size: payload.field_size,
            },
            ErrorCode::HttpResponseIncomplete => Self::HttpResponseIncomplete,
            ErrorCode::HttpResponseHeaderSectionSize(payload) => {
                Self::HttpResponseHeaderSectionSize(payload)
            }
            ErrorCode::HttpResponseHeaderSize(payload) => Self::HttpResponseHeaderSize {
                field_name: payload.field_name,
                field_size: payload.field_size,
            },
            ErrorCode::HttpResponseBodySize(payload) => Self::HttpResponseBodySize(payload),
            ErrorCode::HttpResponseTrailerSectionSize(payload) => {
                Self::HttpResponseTrailerSectionSize(payload)
            }
            ErrorCode::HttpResponseTrailerSize(payload) => Self::HttpResponseTrailerSize {
                field_name: payload.field_name,
                field_size: payload.field_size,
            },
            ErrorCode::HttpResponseTransferCoding(payload) => {
                Self::HttpResponseTransferCoding(payload)
            }
            ErrorCode::HttpResponseContentCoding(payload) => {
                Self::HttpResponseContentCoding(payload)
            }
            ErrorCode::HttpResponseTimeout => Self::HttpResponseTimeout,
            ErrorCode::HttpUpgradeFailed => Self::HttpUpgradeFailed,
            ErrorCode::HttpProtocolError => Self::HttpProtocolError,
            ErrorCode::LoopDetected => Self::LoopDetected,
            ErrorCode::ConfigurationError => Self::ConfigurationError,
            ErrorCode::InternalError(payload) => Self::InternalError(payload),
        }
    }
}

impl From<Error> for ErrorCode {
    fn from(e: Error) -> Self {
        match e {
            Error::DnsTimeout => ErrorCode::DnsTimeout,
            Error::DnsError { rcode, info_code } => {
                ErrorCode::DnsError(types::DnsErrorPayload { rcode, info_code })
            }
            Error::DestinationNotFound => ErrorCode::DestinationNotFound,
            Error::DestinationUnavailable => ErrorCode::DestinationUnavailable,
            Error::DestinationIpProhibited => ErrorCode::DestinationIpProhibited,
            Error::DestinationIpUnroutable => ErrorCode::DestinationIpUnroutable,
            Error::ConnectionRefused => ErrorCode::ConnectionRefused,
            Error::ConnectionTerminated => ErrorCode::ConnectionTerminated,
            Error::ConnectionTimeout => ErrorCode::ConnectionTimeout,
            Error::ConnectionReadTimeout => ErrorCode::ConnectionReadTimeout,
            Error::ConnectionWriteTimeout => ErrorCode::ConnectionWriteTimeout,
            Error::ConnectionLimitReached => ErrorCode::ConnectionLimitReached,
            Error::TlsProtocolError => ErrorCode::TlsProtocolError,
            Error::TlsCertificateError => ErrorCode::TlsCertificateError,
            Error::TlsAlertReceived {
                alert_id,
                alert_message,
            } => ErrorCode::TlsAlertReceived(types::TlsAlertReceivedPayload {
                alert_id,
                alert_message,
            }),
            Error::HttpRequestDenied => ErrorCode::HttpRequestDenied,
            Error::HttpRequestLengthRequired => ErrorCode::HttpRequestLengthRequired,
            Error::HttpRequestBodySize(payload) => ErrorCode::HttpRequestBodySize(payload),
            Error::HttpRequestMethodInvalid => ErrorCode::HttpRequestMethodInvalid,
            Error::HttpRequestUriInvalid => ErrorCode::HttpRequestUriInvalid,
            Error::HttpRequestUriTooLong => ErrorCode::HttpRequestUriTooLong,
            Error::HttpRequestHeaderSectionSize(payload) => {
                ErrorCode::HttpRequestHeaderSectionSize(payload)
            }
            Error::HttpRequestHeaderSize {
                field_name,
                field_size,
            } => ErrorCode::HttpRequestHeaderSize(Some(types::FieldSizePayload {
                field_name,
                field_size,
            })),
            Error::HttpRequestTrailerSectionSize(payload) => {
                ErrorCode::HttpRequestTrailerSectionSize(payload)
            }
            Error::HttpRequestTrailerSize {
                field_name,
                field_size,
            } => ErrorCode::HttpRequestTrailerSize(types::FieldSizePayload {
                field_name,
                field_size,
            }),
            Error::HttpResponseIncomplete => ErrorCode::HttpResponseIncomplete,
            Error::HttpResponseHeaderSectionSize(payload) => {
                ErrorCode::HttpResponseHeaderSectionSize(payload)
            }
            Error::HttpResponseHeaderSize {
                field_name,
                field_size,
            } => ErrorCode::HttpResponseHeaderSize(types::FieldSizePayload {
                field_name,
                field_size,
            }),
            Error::HttpResponseBodySize(payload) => ErrorCode::HttpResponseBodySize(payload),
            Error::HttpResponseTrailerSectionSize(payload) => {
                ErrorCode::HttpResponseTrailerSectionSize(payload)
            }
            Error::HttpResponseTrailerSize {
                field_name,
                field_size,
            } => ErrorCode::HttpResponseTrailerSize(types::FieldSizePayload {
                field_name,
                field_size,
            }),
            Error::HttpResponseTransferCoding(payload) => {
                ErrorCode::HttpResponseTransferCoding(payload)
            }
            Error::HttpResponseContentCoding(payload) => {
                ErrorCode::HttpResponseContentCoding(payload)
            }
            Error::HttpResponseTimeout => ErrorCode::HttpResponseTimeout,
            Error::HttpUpgradeFailed => ErrorCode::HttpUpgradeFailed,
            Error::HttpProtocolError => ErrorCode::HttpProtocolError,
            Error::LoopDetected => ErrorCode::LoopDetected,
            Error::ConfigurationError => ErrorCode::ConfigurationError,
            Error::InternalError(payload) => ErrorCode::InternalError(payload),
        }
    }
}
