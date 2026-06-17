use super::{
    closure_from, handler_trap, is_callable, lookup, revoked_return, revoked_return_with_message,
    POINTER_MASK, POINTER_TAG, PROXIES, TAG_UNDEFINED,
};

/// Return the proxy target for internal-operation callers that must throw on a
/// revoked Proxy instead of treating the small proxy handle like an object
/// pointer.
pub(crate) fn js_proxy_checked_target(proxy_boxed: f64) -> Option<f64> {
    js_proxy_checked_target_with_revoked_message(
        proxy_boxed,
        "Cannot perform operation on a proxy that has been revoked",
    )
}

pub(crate) fn js_proxy_checked_target_for_is_array(proxy_boxed: f64) -> Option<f64> {
    js_proxy_checked_target_with_revoked_message(
        proxy_boxed,
        "Cannot perform 'IsArray' on a proxy that has been revoked",
    )
}

fn js_proxy_checked_target_with_revoked_message(
    proxy_boxed: f64,
    revoked_message: &str,
) -> Option<f64> {
    let id = lookup(proxy_boxed)?;
    PROXIES.with(|p| {
        let borrowed = p.borrow();
        let Some(Some(entry)) = borrowed.get(id as usize) else {
            return None;
        };
        if entry.revoked {
            revoked_return_with_message(revoked_message);
        }
        Some(entry.target)
    })
}

/// Proxy-aware enumerable string key collection for JSON.parse reviver
/// internalization. This intentionally returns the same array-shaped value as
/// `Object.keys` for the no-trap path; a present `ownKeys` trap is invoked so
/// abrupt completions are visible to callers that implement
/// `EnumerableOwnProperties`.
pub(crate) fn js_proxy_own_keys_for_json(proxy_boxed: f64) -> f64 {
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    let (target, handler, revoked) = PROXIES.with(|p| {
        p.borrow()
            .get(id as usize)
            .and_then(|o| o.as_ref())
            .map(|e| (e.target, e.handler, e.revoked))
            .unwrap_or((
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
                false,
            ))
    });
    if revoked {
        return revoked_return();
    }
    let trap = handler_trap(handler, "ownKeys");
    if is_callable(trap) {
        return crate::closure::js_closure_call1(closure_from(trap), target);
    }
    if lookup(target).is_some() {
        return js_proxy_own_keys_for_json(target);
    }
    let keys = crate::object::js_object_keys_value(target);
    f64::from_bits(POINTER_TAG | ((keys as u64) & POINTER_MASK))
}
