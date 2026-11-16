//! Global identifiers and global-object property reads use ordinary shape sites.

use super::*;

use anyhow::Result;

/// Emit the runtime installer a global VALUE read needs (binary size).
///
/// The `process` / `console` namespaces exist from realm bootstrap, but their
/// dynamic-dispatch buckets (and `process`'s stdio stream objects) are
/// installed only on demand, so a program that uses them purely through
/// member intrinsics (`console.log(…)`, `process.argv`) links none of that
/// surface. Every site that can hand one of those objects to user code as a
/// value — the bare global, a method read as a value, or the global object
/// itself (`globalThis` / `global` / `self` / `globalThis[k]`) — runs the
/// matching installer first. Idempotent and cheap (a few relaxed stores).
pub(crate) fn emit_global_value_installs(ctx: &mut FnCtx<'_>, name: &str) {
    let sym = match name {
        "process" => "js_nm_install_process",
        "console" => "js_nm_install_console",
        "globalThis" | "global" | "self" | "window" | "frames" => {
            "js_install_global_value_surfaces"
        }
        _ => return,
    };
    ctx.block().call_void(sym, &[]);
}

/// A builtin global identifier read as a VALUE (`Object`, `Array`, `Date`
/// ...): a static read site of the global object for the name's pooled key.
///
/// The global object is an ordinary object whose own keys are the builtins,
/// so the identifier is `globalThis[name]` with a compile-time key: the
/// generic read site's per-site cache compares the global object's ShapeId
/// and loads the slot, with the key's atom (its pool handle) for the miss.
/// It used to call `js_get_global_this_builtin_value(bytes, len)`, which
/// validated the name's UTF-8, interned it and ran a by-name `[[Get]]` on
/// every evaluation. Reassigning the global (`globalThis.Object = X`) is a
/// store to that object, so the next read sees it: the slot is loaded on
/// every hit, and a key added or deleted changes the ShapeId.
fn global_object_expr(name: &str) -> Expr {
    Expr::Call {
        byte_end: 0,
        callee: Box::new(Expr::ExternFuncRef {
            name: global_entry(name).to_string(),
            param_types: Vec::new(),
            return_type: perry_hir::types::Type::Any,
        }),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

/// Two-phase realm (the default; `PERRY_REALM_TWO_PHASE=0`, read by the
/// compiler, turns it off): the program's entry tells the runtime to populate
/// the realm global in two phases, and a static read of a global name picks
/// its entry at compile time, by the name alone ([`global_entry`]). With the
/// switch off the program populates its realm in one pass at the first touch
/// and does not link the two-phase path.
pub(crate) fn realm_two_phase() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        realm_two_phase_setting(std::env::var("PERRY_REALM_TWO_PHASE").ok().as_deref())
    })
}

/// The switch's reading of its variable: on unless it says off.
fn realm_two_phase_setting(value: Option<&str>) -> bool {
    !matches!(value, Some("0" | "off" | "false"))
}

#[cfg(test)]
mod realm_two_phase_tests {
    #[test]
    fn the_two_phase_realm_is_on_unless_the_switch_says_off() {
        for on in [None, Some("1"), Some("on"), Some("true"), Some("")] {
            assert!(super::realm_two_phase_setting(on), "{on:?}");
        }
        for off in ["0", "off", "false"] {
            assert!(!super::realm_two_phase_setting(Some(off)), "{off}");
        }
    }
}

/// The runtime entry a static read of the global `name` takes its receiver
/// from. The core entry (`js_get_global_core`: phase A's slots are bound)
/// only for a name of `perry_abi::REALM_CORE_GLOBALS`, the declared data names
/// phase A binds; the full entry (`js_get_global_this`: every slot bound) for
/// every other name: one phase B binds, the global itself (`globalThis`,
/// `global`, `self`), an accessor, a name the realm does not declare. The
/// same rule serves the required read and the `typeof` read: both load the
/// name's slot through one shape site, which hits whether or not the slot is
/// bound, so this choice is the only thing between the site and an unbound
/// slot. A store never comes here: it takes the global object as a value,
/// from the full entry.
fn global_entry(name: &str) -> &'static str {
    if realm_two_phase() && (perry_abi::realm_core_global(name) || sabotaged_core(name)) {
        "js_get_global_core"
    } else {
        "js_get_global_this"
    }
}

/// Test-only negative control (`PERRY_FACTS_SABOTAGE=realm_core:<name>`): the
/// compiler reads the phase-B name `<name>` through the core entry, which is
/// the error check mode's poison word exists to report
/// (`scripts/test-verify-facts-s0.py`, arm `realm-phase`).
fn sabotaged_core(name: &str) -> bool {
    static NAME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        std::env::var("PERRY_FACTS_SABOTAGE")
            .ok()
            .and_then(|claim| claim.strip_prefix("realm_core:").map(str::to_string))
    })
    .as_deref()
        == Some(name)
}

/// The value of a static global read. Check mode with a two-phase realm
/// (`PERRY_VERIFY_FACTS=1`): a slot phase B binds is born holding the poison
/// word, and every such read, hit or miss, reports it (rc 86). A release
/// build emits nothing here.
fn checked_global_read(ctx: &mut FnCtx<'_>, value: String) -> String {
    if realm_two_phase() && crate::verify_facts::enabled() {
        ctx.block()
            .call_void("js_realm_check_global_read", &[(DOUBLE, &value)]);
    }
    value
}

pub(crate) fn lower_global_builtin_read(ctx: &mut FnCtx<'_>, name: &str) -> Result<String> {
    let value = super::generic_dispatch::lower_global_property_get(
        ctx,
        &global_object_expr(name),
        name,
        true,
    )?;
    Ok(checked_global_read(ctx, value))
}

/// `typeof` of a global reference permits an absent binding.
pub(crate) fn lower_optional_global_read(ctx: &mut FnCtx<'_>, name: &str) -> Result<String> {
    let value = super::generic_dispatch::lower_global_property_get(
        ctx,
        &global_object_expr(name),
        name,
        false,
    )?;
    Ok(checked_global_read(ctx, value))
}

pub(crate) fn lower_globalget_property(ctx: &mut FnCtx<'_>, property: &str) -> Result<String> {
    crate::verify_facts::family_global_read(ctx, property);
    emit_global_value_installs(ctx, property);
    // GlobalGet is the global object. A builtin member value has a nested
    // receiver read (globalThis.Object, globalThis.Buffer, etc.) and uses the
    // ordinary property path on that value. Never select a builtin here by
    // the member's name: globalThis.isBuffer is an ordinary missing property.
    if is_global_this_builtin_name(property) {
        lower_global_builtin_read(ctx, property)
    } else {
        lower_optional_global_read(ctx, property)
    }
}
