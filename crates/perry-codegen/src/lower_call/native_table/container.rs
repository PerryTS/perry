//! `perry/container`, `perry/compose` and `perry/workloads` dispatch rows
//! (#11211), served by perry-stdlib's `container` feature (the
//! `js_container_*` / `js_compose_*` / `js_workload_*` exports in
//! `crates/perry-stdlib/src/container/`).
//!
//! Before these rows existed HIR lowered every call on these modules to a
//! `NativeMethodCall` that matched nothing here, so it evaluated to
//! `undefined` and never reached the stdlib — the whole JS surface was dead.
//!
//! Signatures follow `types/perry/{container,compose,workloads}/index.d.ts`.
//! `NA_STR` passes a string through and JSON-stringifies an object/array
//! argument (specs, option bags, `cmd: string[]`), which is the JSON the
//! exports parse. Calls whose TS shape does not map 1:1 onto the C ABI
//! (boolean/number flags the exports take as `i32`, option objects that
//! split into several parameters) go through the `*_api_*` adapters in
//! `container/js_api.rs`, which take every argument NaN-boxed (`NA_F64`).
//! `perry/container-compose` is normalized to `perry/compose` in
//! `native_module_lookup`.

use super::*;

const fn row(
    module: &'static str,
    method: &'static str,
    runtime: &'static str,
    args: &'static [NativeArgKind],
    ret: NativeRetKind,
) -> NativeModSig {
    NativeModSig {
        module,
        has_receiver: false,
        method,
        class_filter: None,
        runtime,
        args,
        ret,
    }
}

const C: &str = "perry/container";
const P: &str = "perry/compose";
const W: &str = "perry/workloads";

pub(super) const CONTAINER_ROWS: &[NativeModSig] = &[
    // ── perry/container: single-container lifecycle ──────────────────
    row(C, "run", "js_container_run", &[NA_STR], NR_PROMISE),
    row(C, "create", "js_container_create", &[NA_STR], NR_PROMISE),
    row(C, "start", "js_container_start", &[NA_STR], NR_PROMISE),
    row(
        C,
        "stop",
        "js_container_api_stop",
        &[NA_F64, NA_F64],
        NR_PROMISE,
    ),
    row(
        C,
        "remove",
        "js_container_api_remove",
        &[NA_F64, NA_F64],
        NR_PROMISE,
    ),
    row(C, "list", "js_container_api_list", &[NA_F64], NR_PROMISE),
    row(C, "inspect", "js_container_inspect", &[NA_STR], NR_PROMISE),
    row(
        C,
        "logs",
        "js_container_api_logs",
        &[NA_F64, NA_F64],
        NR_PROMISE,
    ),
    row(
        C,
        "exec",
        "js_container_api_exec",
        &[NA_F64, NA_F64, NA_F64],
        NR_PROMISE,
    ),
    // ── perry/container: images ─────────────────────────────────────
    row(
        C,
        "pullImage",
        "js_container_pullImage",
        &[NA_STR],
        NR_PROMISE,
    ),
    row(C, "listImages", "js_container_listImages", &[], NR_PROMISE),
    row(
        C,
        "removeImage",
        "js_container_api_removeImage",
        &[NA_F64, NA_F64],
        NR_PROMISE,
    ),
    // ── perry/container: compose + cleanup helpers ──────────────────
    row(
        C,
        "composeUp",
        "js_container_composeUp",
        &[NA_STR],
        NR_PROMISE,
    ),
    row(
        C,
        "downByProject",
        "js_container_downByProject",
        &[NA_STR, NA_STR],
        NR_PROMISE,
    ),
    row(C, "downAll", "js_container_downAll", &[NA_STR], NR_PROMISE),
    row(
        C,
        "removeIfExists",
        "js_container_api_removeIfExists",
        &[NA_F64, NA_F64],
        NR_PROMISE,
    ),
    // ── perry/container: backend selection ──────────────────────────
    row(C, "getBackend", "js_container_getBackend", &[], NR_STR),
    row(
        C,
        "detectBackend",
        "js_container_detectBackend",
        &[],
        NR_PROMISE,
    ),
    row(
        C,
        "getAvailableBackends",
        "js_container_getAvailableBackends",
        &[],
        NR_PROMISE,
    ),
    row(
        C,
        "setBackend",
        "js_container_setBackend",
        &[NA_STR],
        NR_PROMISE,
    ),
    row(
        C,
        "setBackends",
        "js_container_setBackends",
        &[NA_STR],
        NR_PROMISE,
    ),
    row(
        C,
        "getBackendPriority",
        "js_container_getBackendPriority",
        &[],
        NR_STR,
    ),
    row(
        C,
        "selectBackendFor",
        "js_container_selectBackendFor",
        &[NA_STR, NA_STR],
        NR_STR,
    ),
    // ── perry/compose ───────────────────────────────────────────────
    // The stack handle `up()` resolves with is passed back NaN-boxed.
    row(P, "up", "js_compose_up", &[NA_STR], NR_PROMISE),
    row(P, "down", "js_compose_down", &[NA_F64, NA_STR], NR_PROMISE),
    row(P, "ps", "js_compose_ps", &[NA_F64], NR_PROMISE),
    row(
        P,
        "logs",
        "js_compose_api_logs",
        &[NA_F64, NA_F64],
        NR_PROMISE,
    ),
    row(
        P,
        "exec",
        "js_compose_exec",
        &[NA_F64, NA_STR, NA_STR],
        NR_PROMISE,
    ),
    row(P, "config", "js_compose_config", &[NA_F64], NR_PROMISE),
    row(
        P,
        "start",
        "js_compose_start",
        &[NA_F64, NA_STR],
        NR_PROMISE,
    ),
    row(P, "stop", "js_compose_stop", &[NA_F64, NA_STR], NR_PROMISE),
    row(
        P,
        "restart",
        "js_compose_restart",
        &[NA_F64, NA_STR],
        NR_PROMISE,
    ),
    // ── perry/workloads (alpha) ─────────────────────────────────────
    // `runtime` / `policy` are helper-constructor objects with no FFI
    // export; they are not rows here.
    row(W, "graph", "js_workload_graph", &[NA_STR, NA_STR], NR_STR),
    row(W, "node", "js_workload_node", &[NA_STR, NA_STR], NR_STR),
    row(
        W,
        "runGraph",
        "js_workload_runGraph",
        &[NA_STR, NA_STR],
        NR_PROMISE,
    ),
    row(
        W,
        "inspectGraph",
        "js_workload_api_inspectGraph",
        &[NA_F64],
        NR_PROMISE,
    ),
];

#[cfg(test)]
mod tests {
    use super::super::super::native_module_lookup;
    use perry_api_manifest::{ApiKind, API_MANIFEST};

    const MODULES: &[&str] = &[
        "perry/container",
        "perry/compose",
        "perry/container-compose",
        "perry/workloads",
    ];

    /// #11211: every function these modules declare must resolve to a
    /// dispatch row. A declared-but-unrouted call lowers to `undefined`
    /// silently, which is how the whole surface went dead unnoticed.
    #[test]
    fn every_declared_function_resolves_to_a_dispatch_row() {
        let mut checked = 0;
        let mut missing = Vec::new();
        for entry in API_MANIFEST.iter() {
            if !MODULES.contains(&entry.module) {
                continue;
            }
            let ApiKind::Method {
                has_receiver: false,
                class_filter: None,
            } = entry.kind
            else {
                continue;
            };
            checked += 1;
            if native_module_lookup(entry.module, false, entry.name, None).is_none() {
                missing.push(format!("{}.{}", entry.module, entry.name));
            }
        }
        assert!(checked >= 40, "only {checked} manifest functions seen");
        assert!(missing.is_empty(), "no dispatch row for: {missing:?}");
    }

    #[test]
    fn container_compose_alias_shares_the_compose_rows() {
        let alias = native_module_lookup("perry/container-compose", false, "up", None).unwrap();
        assert_eq!(alias.runtime, "js_compose_up");
    }
}
