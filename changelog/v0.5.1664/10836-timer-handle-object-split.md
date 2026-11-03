`crates/perry-runtime/src/timer.rs` reached 2345 lines and failed
`scripts/check_file_size.sh`'s 2000-line cap.

Moved the #340/#341 handle-object surface — the whole of it and nothing else —
into the new sibling `timer/handle_object.rs`: the `Timeout` / `Immediate`
class ids, the packed `ObjectMeta.native_state` word and its accessors
(`timer_state_word`, `timer_handle_parts`, `timer_handle_id`), the two
per-realm prototype singletons and `scan_timer_prototype_roots_mut`, the
prototype and constructor installers (`build_timer_prototypes`,
`install_timer_symbol_method`, `install_timer_constructor`,
`timer_prototype`), the shared receiver plumbing (`timer_receiver`,
`throw_timer_type_error`, `clear_every_kind`) and the allocator
`timer_object`.

What stays in `timer.rs` is the scheduler: queues, tick loops, the `js_set_*` /
`clear*` entry points and the mock-timer surface. The seam is two calls wide —
the scheduler wraps a freshly minted id with `timer_object(id, kind)` and reads
one back with `timer_handle_id` / `timer_handle_parts` — which is why this is
the boundary rather than a line cut. `timer.rs` lands at 1958, the new module
at 412.

Re-exports are explicit and named. `timer_handle_parts` and `TIMEOUT_CLASS_ID`
are reached by `crate::timer::`-qualified paths from unit tests only, so their
re-export is `#[cfg(test)]` — unconditional, it is an unused import in a lib
build and `-D warnings` rejects it.

The raw-handle ledger is unaffected: all 7 of `timer.rs`'s recorded debt sites
stay in `timer.rs` (`handle_object.rs` has none), so no ceiling moved and no
`# moved-from:` annotation is needed.
