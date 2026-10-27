// Error vocabulary adapted from Wasmtime 48.0.5 wasi-http.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// See PROVENANCE.md and LICENSE.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    HttpRequestBodySize(Option<u64>),

    DnsTimeout,
    DnsError {
        rcode: Option<String>,
        info_code: Option<u16>,
    },
    DestinationNotFound,
    DestinationUnavailable,
    DestinationIpProhibited,
    DestinationIpUnroutable,
    ConnectionRefused,
    ConnectionTerminated,
    ConnectionTimeout,
    ConnectionReadTimeout,
    ConnectionWriteTimeout,
    ConnectionLimitReached,
    TlsProtocolError,
    TlsCertificateError,
    TlsAlertReceived {
        alert_id: Option<u8>,
        alert_message: Option<String>,
    },
    HttpRequestDenied,
    HttpRequestLengthRequired,
    HttpRequestMethodInvalid,
    HttpRequestUriInvalid,
    HttpRequestUriTooLong,
    HttpRequestHeaderSectionSize(Option<u32>),
    HttpRequestHeaderSize {
        field_name: Option<String>,
        field_size: Option<u32>,
    },
    HttpRequestTrailerSectionSize(Option<u32>),
    HttpRequestTrailerSize {
        field_name: Option<String>,
        field_size: Option<u32>,
    },
    HttpResponseIncomplete,
    HttpResponseHeaderSectionSize(Option<u32>),
    HttpResponseHeaderSize {
        field_name: Option<String>,
        field_size: Option<u32>,
    },
    HttpResponseBodySize(Option<u64>),
    HttpResponseTrailerSectionSize(Option<u32>),
    HttpResponseTrailerSize {
        field_name: Option<String>,
        field_size: Option<u32>,
    },
    HttpResponseTransferCoding(Option<String>),
    HttpResponseContentCoding(Option<String>),
    HttpResponseTimeout,
    HttpUpgradeFailed,
    HttpProtocolError,
    LoopDetected,
    ConfigurationError,
    InternalError(Option<String>),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}
