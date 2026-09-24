//! #10468 — client-side protocol upgrade (`Connection: Upgrade`). A `101
//! Switching Protocols` response hands the caller the raw socket through
//! `req.on('upgrade', (res, socket, head) => ...)` instead of an ordinary
//! `'response'`.
//!
//! The exchange runs in `client_turnloop` (`Mode::Upgrade`): the codec reports
//! the `101` as `Event::Upgrade`, and the live turnloop handle is handed to
//! `perry_ext_net` with `turnloop_net::transfer` — the server's `'upgrade'`
//! handoff, from the client side — so no descriptor moves and no byte is lost.
//! This module used to speak the request over a raw tokio `TcpStream` because
//! reqwest never exposed the connection. What remains is the predicate every
//! path agrees on.

use std::collections::HashMap;

/// `true` if `headers` asks for a protocol upgrade — `Connection: Upgrade`
/// as one token of a comma list (RFC 7230 §6.1; Node/undici send it as a
/// bare `Upgrade` value in practice).
pub(crate) fn wants_upgrade(headers: &HashMap<String, String>) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("connection")
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("upgrade"))
    })
}
