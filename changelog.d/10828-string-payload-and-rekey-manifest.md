Two lint-tier gate fixes the descriptor_state split and the new
`object/shape_rule3.rs` require. Neither is a baseline bump.

**`string_payload_access_inventory.py`** went 350 → 351 `inline-offset` sites in
`perry-runtime`. The new one is `shape_rule3.rs`'s `thrown_message` test helper,
which open-coded the payload offset:

    let data = (message as *const u8).add(size_of::<StringHeader>());

It is a copy of a pattern the `global_this_webassembly` tests use, and the
ratchet exists to stop that pattern spreading. Converted to the runtime's own
`crate::string::string_data(message)` — the accessor `header_str_checked` and
the rest of `string/mod.rs` already go through — which returns the count to 350.
Confirmed the +1 was this train's before touching anything: `origin/main` is
green on the gate.

**`gc_rekeyed_key_tables.py`** reported both halves of a moved site:

    UNCLASSIFIED rekey site  .../descriptor_state/owner_lifecycle.rs::rewrite_descriptor_owner
    STALE manifest entry     .../descriptor_state.rs::rewrite_descriptor_owner

The manifest is path-keyed and `rewrite_descriptor_owner` moved in the
2000-line-cap split. Re-pointed rather than re-classified, because the custody
argument is unchanged and that was checked rather than assumed: the moved
function body is **byte-identical** apart from its visibility widening to
`pub(super)` (`diff` of the two extents shows exactly one differing line). The
entry's `why` also cited `descriptor_state.rs:1035-1055` for the registered
prune, which moved with it; that now names
`descriptor_state/owner_lifecycle.rs:34-52`, so the reference stays checkable
instead of merely passing.

The gate refusing the *stale* entry as well as the new site is the behaviour
that makes this safe — a fix cannot quietly leave its own exemption behind.
