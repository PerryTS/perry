Exercise `IncomingMessage.pipe()` through generic method dispatch in both
auto-optimized and `PERRY_NO_AUTO_OPTIMIZE=1` integration-test modes. This adds
coverage related to #10670; it does not resolve the underlying behavior gap.
