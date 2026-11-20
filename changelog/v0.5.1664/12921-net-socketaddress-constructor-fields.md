### Fix `net.SocketAddress` constructor field validation and defaults

- Preserve raw field types in the existing moving-GC-aware roots and read option getters once, in `family`, `address`, `port`, `flowlabel` order, before validation. Align family/address type errors and invalid-family error codes with Node; retain the existing uint32 range validation for `flowlabel`.
- Select an omitted address from the raw family before lowercase normalization, including the `::` default for `ipv6`. Preserve top-level option admission, existing port acceptance, the parser, and shared `BlockList` semantics.
- Verify the new 11-row constructor-field regression and unchanged 13-row validation fixture against Node 26.5.1, both failing before the fix and passing afterward. This is a bounded constructor-field fix, not an implementation of the missing static `isSocketAddress` method or completion of the `net` module.
