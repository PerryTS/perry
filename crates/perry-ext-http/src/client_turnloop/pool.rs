//! Pure keep-alive policy and compatibility keys for actual Agent Socket edges.
//!
//! There is no physical connection pool here. `agent.rs` owns active/free
//! Socket objects. Framing parks one only after a complete reusable response;
//! its unreferenced core deadline, peer EOF or unsolicited data releases it.

use perry_ffi::Handle;

/// What a request's agent allows. Read on the JS thread at dispatch, from the
/// same `AgentHandle` fields reqwest's per-agent client was built from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Reuse {
    /// Idle connections kept per key (`maxFreeSockets`).
    pub(crate) max_free: usize,
    /// How long an idle connection is kept (`keepAliveMsecs`).
    pub(crate) idle_ms: u64,
}

/// The reuse policy for an agent, or `None` when it must not reuse.
///
/// `agent_handle == 0` is `http`'s implicit global agent, which the reqwest
/// path served from one shared client; the turnloop lane has carried it with a
/// connection per request since lane 1, and that is kept.
pub(crate) fn policy_for(agent_handle: Handle) -> Option<Reuse> {
    if agent_handle == 0 {
        return None;
    }
    let (keep_alive, max_free_sockets, keep_alive_msecs) =
        crate::agent::agent_pool_config(agent_handle)?;
    policy_from(keep_alive, max_free_sockets, keep_alive_msecs)
}

/// The arithmetic `client_for_agent` fed reqwest's pool, kept exactly.
pub(crate) fn policy_from(
    keep_alive: bool,
    max_free_sockets: f64,
    keep_alive_msecs: f64,
) -> Option<Reuse> {
    if !keep_alive {
        return None;
    }
    let max_free = if !max_free_sockets.is_finite() || max_free_sockets > usize::MAX as f64 {
        256
    } else {
        max_free_sockets.max(1.0) as usize
    };
    let idle_ms = if keep_alive_msecs.is_finite() && keep_alive_msecs > 0.0 {
        keep_alive_msecs as u64
    } else {
        1000
    };
    Some(Reuse { max_free, idle_ms })
}

/// Which connections are interchangeable. Anything that changes the peer, the
/// TLS identity, or the agent that owns the socket separates them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PoolKey {
    pub(crate) agent: Handle,
    pub(crate) https: bool,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) proxy: Option<String>,
    pub(crate) tls: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_alive_false_never_reuses() {
        assert_eq!(policy_from(false, 256.0, 1000.0), None);
    }

    #[test]
    fn the_reqwest_pool_arithmetic_is_kept() {
        assert_eq!(
            policy_from(true, f64::INFINITY, 0.0),
            Some(Reuse {
                max_free: 256,
                idle_ms: 1000
            })
        );
        assert_eq!(
            policy_from(true, 0.0, 250.0),
            Some(Reuse {
                max_free: 1,
                idle_ms: 250
            })
        );
        assert_eq!(
            policy_from(true, 4.0, f64::NAN).map(|r| r.idle_ms),
            Some(1000)
        );
    }

    #[test]
    fn the_implicit_http_agent_does_not_reuse() {
        assert_eq!(policy_for(0), None);
    }
}
