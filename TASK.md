# Fix the typed-array aliasing regression in PR #9360

## The bug

`main` is correct. `main` + PR #9360 turns a `Uint8Array` view over another
typed array's buffer into mostly zeros.

```ts
const words = new Uint32Array(2);
const bytes = new Uint8Array(words.buffer);
words[0] = 0x01020304;
words[1] = 0x05060708;
for (let i = 0; i < 8; i++) out.push(bytes[i]);
```

| | result |
|---|---|
| node | `4 3 2 1 8 7 6 5` |
| perry + #9360 | `4 0 0 0 0 0 0 0` |

It also fails the committed fixture
`test-files/test_gap_typedarray_buffer_aliasing_7219.ts`, which is #7219's own
regression test — that fixture passes on `main` and fails with this PR.

## What is already established — do NOT re-derive these

1. **Culprit is one commit.** `main` + `7de78d0576` ALONE reproduces it. The six
   perf commits stacked on top are not implicated.
2. **The write is fine.** After `words[0] = 0x01020304`, reading `words[0]` back
   gives `16909060` exactly. The u32 store landed correctly.
3. **The metadata is fine.** `bytes.byteLength == 4`, `bytes.length == 4`,
   `words.buffer.byteLength == 4` — all match node.
4. **It is NOT a stride error.** Reading `base + i*4` would give `4 8 0 0 …` on
   the two-word case (index 4 hitting `words[1]`'s low byte). Actual output is
   `4 0 0 0 0 0 0 0`, so that hypothesis is disproved.
5. **The codegen lowering is excluded by symbol evidence**, not inference:
   `nm` on the compiled fixture shows ZERO references to `js_u8_buffer_read_f64`.
   `try_lower_u8_buffer_read` never fires here. (An earlier `PERRY_U8_INLINE_READ=0`
   A/B "ruling it out" was vacuous for the same reason — the path was never taken,
   so the switch had nothing to disable. Do not repeat that experiment.)

So: element 0 reads correctly and every other index reads 0, while length and
byteLength are right. Live surface is the runtime side of `7de78d0576`:
`crates/perry-runtime/src/typedarray/access.rs` (+71) and
`crates/perry-runtime/src/buffer/header.rs` (+67).

## Fixes already ATTEMPTED AND FAILED — do not repeat

- Evicting the stale `PERRY_U8_INLINE_CACHE` admission in `register_view_meta`.
- Guarding both registry-miss recovery arms with `view_meta_of(addr).is_none()`.
- Narrowing both recovery arms from `is_registered_buffer(addr)` to
  `is_uint8array_buffer(addr)`.

None changed the output. Note the last two were never confirmed to be REACHED
for this receiver — if you use them as evidence, first prove the arm executes
(add a temporary eprintln or counter), or you will repeat a vacuous experiment.

## Build and test

```
cd /Users/amlug/projects/perry/cx-9360
export CARGO_TARGET_DIR=/Users/amlug/agent-targets/cx9360
export PERRY_RUNTIME_DIR=$CARGO_TARGET_DIR/release
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
$CARGO_TARGET_DIR/release/perry test-files/test_gap_typedarray_buffer_aliasing_7219.ts -o /tmp/cx9360_fix
diff <(node --experimental-strip-types test-files/test_gap_typedarray_buffer_aliasing_7219.ts) <(/tmp/cx9360_fix)
```

Node must be v26.5.1 (matches `.node-version`). A build is ~8-10 minutes; the
compile+run of one fixture is seconds, so iterate on the fixture, not the build.

## Definition of done

1. The fixture above is byte-identical to node.
2. `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime` is green
   (perry-runtime tests are NOT parallel-safe; the flag is required).
3. `./scripts/check_file_size.sh`, `python3 scripts/raw_handle_debt.py`,
   `python3 scripts/gc_runtime_root_holders.py`, `python3 scripts/addr_class_inventory.py`
   and `cargo fmt --all -- --check` all pass.
4. The fix keeps #9360's actual feature working — it exists to recover
   `Uint8Array` elements on a kind-registry miss. Do not fix the aliasing bug by
   deleting the feature; if the recovery must narrow, say precisely which
   receivers it still serves.

## Rules

- Do not touch `test-parity/gap_snapshot.json` or any baseline/allowlist file to
  make a gate pass. Fix the code.
- Do not raise a ratchet ceiling.
- Report what you changed and WHY, and state any claim you could not verify.
