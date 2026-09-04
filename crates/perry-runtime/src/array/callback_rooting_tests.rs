//! #9673 ratchet: every `Array.prototype` higher-order arm must dispatch a
//! callback it re-read from a GC root, never the raw pointer it was handed.
//!
//! The defect this pins: a callback born at the call site (the inline arrow in
//! `xs.forEach(x => …)`) is reachable ONLY through that raw parameter plus the
//! native stack, which an evacuating minor does not scan. Every dispatch in the
//! loop allocates, so binding the address once and reusing it dereferences a
//! closure the collector has already retired — the read lands on recycled
//! memory whose header is no longer `CLOSURE_MAGIC`, and the next validation
//! reports the recycled object's `typeof`: `TypeError: object is not a
//! function`.
//!
//! `js_array_map` (#6081/#6206), `js_array_filter` and `js_array_map_discard`
//! (#7533) each learned this one arm at a time, three separate times, because
//! nothing checked the others. This is the check: it reads the module's own
//! source, so a new arm — or a refactor that drops a re-read — fails here
//! instead of waiting for a collection to land in its window.

const SRC: &str = include_str!("iter_methods.rs");

fn lines() -> Vec<&'static str> {
    SRC.lines().collect()
}

/// Indices (0-based) of every line that hands a callback to a resolved
/// direct-call site.
fn dispatch_indices(lines: &[&str]) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("cb_site.call("))
        .map(|(i, _)| i)
        .collect()
}

/// The argument a dispatch actually passes: normally on the same line, but the
/// `reduce` arm wraps, so a bare `cb_site.call(` takes the line below.
fn callee_argument<'a>(lines: &[&'a str], idx: usize) -> &'a str {
    let line = lines[idx];
    if line.trim_end().ends_with("cb_site.call(") {
        lines.get(idx + 1).copied().unwrap_or("")
    } else {
        line
    }
}

/// A dispatch is rooted when its callee argument is a re-read of the loop's
/// root: the `current_callback()` closure over a NaN-boxed handle
/// (`js_array_map_discard` and the #9673 arms), or a `callback` shadowed from
/// the raw handle immediately above (`js_array_map`, `js_array_filter`).
fn is_rooted_dispatch(lines: &[&str], idx: usize) -> bool {
    let arg = callee_argument(lines, idx);
    if arg.contains("current_callback()") {
        return true;
    }
    if arg.contains("callback") {
        let start = idx.saturating_sub(4);
        return lines[start..idx]
            .iter()
            .any(|l| l.contains("let callback = cb_handle."));
    }
    false
}

#[test]
fn every_dispatch_reads_the_callback_from_its_root() {
    let lines = lines();
    let offenders: Vec<String> = dispatch_indices(&lines)
        .into_iter()
        .filter(|idx| !is_rooted_dispatch(&lines, *idx))
        .map(|idx| format!("iter_methods.rs:{}: {}", idx + 1, lines[idx].trim()))
        .collect();
    assert!(
        offenders.is_empty(),
        "#9673: these Array.prototype arms dispatch the raw callback parameter \
         instead of re-reading it from the loop's root — a collection inside the \
         loop retires the closure and the next validation throws \
         `object is not a function`:\n  {}",
        offenders.join("\n  ")
    );
}

/// The dispatch check is only meaningful if the arms actually root the
/// callback. Every `DirectCallN::resolve(callback)` must be followed, inside
/// its own function, by a root of that same callback.
#[test]
fn every_resolved_call_site_roots_its_callback() {
    let lines = lines();
    let mut offenders = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !line.contains("::resolve(callback)") {
            continue;
        }
        let mut rooted = false;
        for probe in lines.iter().skip(i + 1) {
            if probe.starts_with("pub extern") || probe.starts_with("fn ") {
                break;
            }
            if probe.contains("root_raw_const_ptr(callback)")
                || probe.contains("js_nanbox_pointer(callback as i64)")
            {
                rooted = true;
                break;
            }
        }
        if !rooted {
            offenders.push(format!("iter_methods.rs:{}: {}", i + 1, line.trim()));
        }
    }
    assert!(
        offenders.is_empty(),
        "#9673: these loops resolve a direct-call site for a callback they never \
         root — the closure can be retired mid-loop:\n  {}",
        offenders.join("\n  ")
    );
}

/// Non-vacuity: the scan must see every arm, and it must reject the shape the
/// fix removed.
#[test]
fn the_ratchet_sees_every_arm_and_rejects_the_old_shape() {
    let lines = lines();
    assert!(
        dispatch_indices(&lines).len() >= 12,
        "expected every Array.prototype higher-order arm to be scanned, saw {}",
        dispatch_indices(&lines).len()
    );
    for arm in [
        "js_array_forEach",
        "js_array_map",
        "js_array_filter",
        "js_array_find",
        "js_array_some",
        "js_array_every",
        "js_array_flatMap",
        "js_array_reduce",
    ] {
        assert!(SRC.contains(arm), "{arm} is no longer in iter_methods.rs");
    }
    // The pre-fix shape — a bare `cb_site.call(callback, …)` with no rebind
    // above it — must be rejected, or the scan above proves nothing.
    let pre_fix = [
        "    let cb_site = X::resolve(callback);",
        "    cb_site.call(callback, a, b, c);",
    ];
    assert!(
        !is_rooted_dispatch(&pre_fix, 1),
        "the ratchet accepts the shape #9673 removed"
    );
}
