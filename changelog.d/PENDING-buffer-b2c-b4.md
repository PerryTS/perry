Route native byte consumers through scoped byte access or owner-thread pins. Crypto, WebCrypto, SQLite, TLS, HTTP/2, ethers, web streams/BYOB, StringDecoder, querystring, filesystem results, TextEncoder/TextDecoder, V8/VM and structured-clone copies no longer derive byte addresses from buffer headers. `copy_value` roots a source before destination allocation and resolves its bytes afterward.

Compatible B4 changes move detached state into the owner header (deleting `DETACHED_BUFFER_REGISTRY` and `EVER_DETACHED`), replace `INLINE_OWNING_U32_CACHE` with a current-header admission check, and prefix all three persistent-symbol factories with an eight-byte leaf GC header. Detached state uses bit 14, leaving bits 3–5 for GC survival age; the existing bounded pin counter uses bits 9–13 (31 simultaneous pins per owner, returning `PinLimit` on overflow). The source gate also checks old raw byte-helper calls and emitted offsets; `run_lint_gates.sh` discovers it through the existing lint workflow. Child-process sabotages exercise the detector and the runtime/consumer contracts.

Boundary still pending: generated code directly links `PERRY_U8_INLINE_CACHE` and `PERRY_TA_KIND_CACHE`, and reads typed-array metadata at +8/+10. The proposed unified layout puts a data/owner pointer at +8. Removing those exported caches or replacing those fields requires the B4c codegen switch, explicitly excluded from this lane until #12023. The six-table deletion and unified 16-byte layout therefore cannot all be completed within that boundary. No placement policy, versions, fetch bodies, zlib implementation, or runtime node_stream files are changed.

The exact closed census rows are in `scripts/buffer_b4_census_closed.tsv`: 73 rows (42 runtime, 27 stdlib, 2 updater, 1 ext-http, 1 ext-net), comprising 35 creation, 28 size-assumption, 8 unscoped-borrow and 2 pointer-across-GC sites. Other ext producers already use B1's C ABI wrapper. The expanded source gate has 282 existing sites and zero additions; the same detector found 444 at the earlier B1 baseline, with 161 removed by this lane and one by upstream #12094. It is a ratchet, not the still-pending zero-debt layout invariant.

| Machinery | Deleted | Still pending |
|---|---|---|
| Address-keyed tables | `DETACHED_BUFFER_REGISTRY` | `VIEW_REGISTRY`, `BACKING_TO_VIEWS`, `RESIZABLE_BUFFER_MAX`, `BUFFER_AB_ALIAS`, `TYPED_ARRAY_VIEW_META` |
| Latches | `EVER_DETACHED` | `RESIZABLE_BUFFER_EVER_MARKED`, `BUFFER_AB_ALIAS_EVER_SET` |
| Address caches | `INLINE_OWNING_U32_CACHE` | `PERRY_U8_INLINE_CACHE`, `PERRY_TA_KIND_CACHE` |

| Contract | Witness and sabotage | Result |
|---|---|---|
| T1/T2: moving collection; borrow allocation forbidden | B1 byte tests; seven forced-GC byte tests | PASS; B1 child witnesses abort on violation |
| T3: detach during operation | Deflate data listener transfers the input owner; native last-pin lifetime witness with B1 `detach_free` and B4 `detach_mark` sabotages | Node parity PASS on main; final head/ASan pending |
| T4: worker transfer | 32 MiB transfer preserves backing pointer, sender length zero, receiver contents and backing count; `transfer_copy` | PASS / RED |
| T5: large concat and nested views | 300 × 18,000 bytes; only the nested view roots its owner across minor and full GC; `view_edge` | PASS / RED; placement counter belongs to B3 |
| T6: owner access checks | u8, i32 and DataView share writes; shrink/OOB/grow/detach; `owner_check` | PASS / RED |
| T7: native outputs | Crypto sizes 0, 1, 255, 256, 257 and 1 MiB; random fill bounds; `crypto_output`, `crypto_borrow`, `random_fill_range`; B1 C ABI producer contracts | PASS / RED; production threshold policy is unchanged |
| T8: Buffer parameter view | Main's `test_gap_12094_buffer_param_views` | Main PASS; final head pending |
| T9: whole-module invariant | Source gate; owner-edge witness; header and root-holder gates | Ratchet PASS, nine source sabotages RED; address-table-free invariant pending B4c |
| Symbol header and u32 admission | Three persistent-symbol factories; current-header u32 admission; `symbol_header`, `u32_admission` | PASS / RED |

Verification baseline: origin/main `2fb54a09942bf26766095995f8a5dc74d10b18f6`; refreshed/rebased B1 `43cb95a83bec59c463461ff0d6d1ffca407c888a`. Production sources were unchanged by that B1 refresh. The newer main changes RegExp, exception snapshots and getter memos, so the final build/test/output/performance comparison is being rerun against that exact snapshot. Final verification after routing symbol pins through the shared setter is pending. The two stdlib thread-exit failures reproduce on both arms; no new failures were observed in the earlier complete 54-binary comparison. The full lint tier is not claimed: the file-size gate has three unchanged main violations (`dynamic_dispatch.rs`, `delete_rest.rs`, `method_site.rs`).

Final program/kernel measurements: pending. The additional eight bytes per small Buffer have not been introduced by this compatible subset; their RSS effect is unmeasured and belongs to the unified-layout change. Persistent symbols gained eight bytes each.
