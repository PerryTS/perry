# Arena trigger promotion assertion follow-up

Implementation SHA: `0fa6101bbcca31605b0bdad0e2c3740f19149a74`

Branch: `perf/arena-trigger-excludes-nursery` (push target: `fork` only)

## Root cause

This is case (b): the in-place promotion path deliberately reserves one fresh
block after transferring the filled nursery blocks.

- `crates/perry-runtime/src/arena/promote.rs:338-352` installs every captured
  in-use nursery block into `OLD_ARENA`, adds its used bytes to old-gen
  occupancy, and subtracts the same reserved capacity from
  `NURSERY_RESERVED_BYTES`. It does not change `ARENA_TOTAL_BYTES`; that part
  is a capacity transfer.
- The isolated fixture starts with the reusable empty Eden head left by its
  preparatory minor. `fill_dead_nursery_to(16 MiB + BLOCK_SIZE)` consumes that
  head and allocates until all 18 reserved 1 MiB Eden blocks have nonzero
  offsets. `retag_young_for_in_place_promotion` therefore captures all 18.
- `crates/perry-runtime/src/arena/promote.rs:563-587` must leave Eden with a
  usable allocator head. After all 18 filled blocks become tombstones, it calls
  `install_fresh_block(BLOCK_SIZE)`.
- `crates/perry-runtime/src/arena/block.rs:654-689` accounts that new nursery
  block through `arena_reserved_bytes_add`; `block.rs:1044-1049` correctly adds
  1 MiB to both `ARENA_TOTAL_BYTES` and `NURSERY_RESERVED_BYTES`.

The reported numbers account exactly: 18 MiB before promotion becomes 19 MiB
whole-arena capacity after promotion, comprising 18 MiB old space plus the
mandatory empty 1 MiB Eden head. Because that head is present in both total and
nursery counters, it cancels out of `old_space_total_bytes()`. No nursery
counter creation/reset/release arm is missing.

The failing assertion was therefore wrong at
`crates/perry-runtime/src/gc/tests/arena_trigger_old_space.rs:152-157`: promotion
transfers all existing capacity, but restoring the required Eden allocator head
necessarily reserves one new block.

## Base-difference audit

`git log 504e180d0..616a2cb84 -- crates/perry-runtime/src/arena \
crates/perry-runtime/src/gc/policy.rs crates/perry-runtime/src/gc/promote.rs \
crates/perry-runtime/src/gc/reset.rs` returned no commits. The one-block result
is not caused by drift between perrymaster's `504e180d0` base and this branch's
`616a2cb84` base.

## Change

`crates/perry-runtime/src/gc/tests/arena_trigger_old_space.rs:133-167` now:

- proves the fixture promotes every reserved nursery block, making the fresh
  Eden head deterministic;
- expects whole-arena reservation to increase by exactly `BLOCK_SIZE` and
  identifies that block in the assertion message;
- verifies the post-promotion nursery reservation is exactly the replacement
  Eden head; and
- verifies old-space pacing increases by exactly the promoted reservation, so
  the replacement nursery head remains excluded.

Runtime bookkeeping was not changed because its total, nursery, and old-space
quantities are already internally consistent.

## Validation

Every Cargo invocation was preceded by `df -g /`; free space was 27-65 GB,
above the binding 12 GB floor. Cargo ran through
`cc-perf-campaign/measure_lock.sh --build` with `-j4`.

- Exact test: passed (1 passed, 0 failed).
- Runtime lib suite, non-PTY and single-threaded: passed (3261 passed, 0
  failed, 4 ignored).
- Archive feature-set build: `cargo build --release -j4 -p perry-runtime
  --features wasm-host` passed.
- Default compiler build: `cargo build --release -j4 -p perry` passed.
- `rustfmt --edition 2021` on the changed Rust file: passed.
- `git diff --check`: passed.
- `bash scripts/check_file_size.sh`: passed.

One earlier runtime-suite attempt was launched through a PTY and produced 3260
passed / 1 failed / 4 ignored; the sole failure was
`tty::tests::columns_undefined_when_not_tty`, because the runner had supplied a
TTY. The required non-PTY rerun above was fully green.

Not run: a separate `nm` audit or the external performance campaign. This
follow-up changes only test assertions; the prescribed runtime, feature-set,
and compiler gates all ran locally.
