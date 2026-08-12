### `PERRY_STACKMAP_WALKER=verify` now names the disagreement it finds

The first end-to-end `verify` run on aarch64 ELF caught the fp-chain walker and
the Itanium unwinder resolving the same GC root 96 bytes apart (#7984). The
whole of what the gate could report was `fast walk visited 1 unique slots,
unwinder visited 1` and the two addresses in decimal — not the frame, not the
base register, not the function whose prologue was decoded, and therefore not
*which walker is wrong*. Every candidate explanation predicts exactly that
output: a `sub sp` the prologue decoder's contiguous-run rule missed, a frame
the x29 chain skipped because an intermediate frame carries no frame record
(legal on Linux, not on Darwin), or a CFA one frame out on libgcc.

Both walkers now hand back a `ResolvedRoot` rather than a bare
`MutableRootSlot`: the same address, plus the frame return address it was
matched on, the record's function, the map's base register and frame offset,
and the base that walker resolved that register to.
`visit_stack_map_root_slots` projects it straight back to a `MutableRootSlot`,
so the collector's view is unchanged. On a mismatch `verify` prints every root
from both walks, states that an equal slot count means a *base* disagreement
rather than a missed frame (with the per-slot byte delta), and on aarch64 dumps
`fp_to_sp_offset`'s decode together with the prologue words it read — the
ground truth for the frame layout the fast walker derives an SP base from.

The prologue dump is gated on the parsed map vouching for the function address
(`function_starts`, the same set `match_records` consults). The first draft was
not gated, and a unit test with a synthetic address turned the diagnostic into
a SIGSEGV with no output — which is what would happen in the field for the one
failure mode where a report matters most, a map whose addresses are wrong.
`an_unvouched_function_address_is_never_dereferenced` pins it.

`gc-native-roots.yml`'s crash path tailed 20 lines of the failing run's stderr,
which truncates the report's head; it now tails 120. `verify` and the decoder
tests move into `stack_maps_verify.rs` and `stack_maps_decode_tests.rs` because
`stack_maps.rs` was eight lines under the 2000-line cap.

This does not fix #7984 — the `ubuntu-24.04-arm` arm stays red. It makes that
arm's next red run diagnostic instead of a riddle.
