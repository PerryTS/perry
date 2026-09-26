//! Fetch destination-port policy, shared by preflight and redirected hops.

/// Refuse HTTP(S) destinations on the Fetch standard's bad-port list.
/// https://fetch.spec.whatwg.org/#port-blocking
///
/// URL parsing normalizes leading zeroes and removes explicit default ports;
/// the HTTP(S) defaults (80/443) are allowed. This is a destination policy,
/// independent of the port used to reach a configured proxy.
pub(super) fn check(url: &url::Url) -> Result<(), turnloop_http::Error> {
    const BAD_PORTS: &[u16] = &[
        0, 1, 7, 9, 11, 13, 15, 17, 19, 20, 21, 22, 23, 25, 37, 42, 43, 53, 69, 77, 79, 87, 95,
        101, 102, 103, 104, 109, 110, 111, 113, 115, 117, 119, 123, 135, 137, 139, 143, 161, 179,
        389, 427, 465, 512, 513, 514, 515, 526, 530, 531, 532, 540, 548, 554, 556, 563, 587, 601,
        636, 989, 990, 993, 995, 1719, 1720, 1723, 2049, 3659, 4045, 4190, 5060, 5061, 6000, 6566,
        6665, 6666, 6667, 6668, 6669, 6679, 6697, 10080,
    ];
    if matches!(url.scheme(), "http" | "https")
        && url.port().is_some_and(|port| BAD_PORTS.contains(&port))
    {
        return Err(turnloop_http::Error::new("ERR_BAD_PORT", "bad port"));
    }
    Ok(())
}
