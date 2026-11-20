- **runtime/gc: the old-gen hole free list is threaded through the holes (#11505).** `OLD_FREE_MAP` was a thread-local map from hole size to a `Vec` of hole addresses, emptied and refilled by every old-reclaiming sweep. Each swept hole cost a `Vec` push, with that vector's growth reallocations, and each rebuild freed the vectors and grew them again. The list is now intrusive: a listed hole's first payload word holds the user pointer of the next hole of the same size, and the only state outside the heap is one head per size — a direct-indexed array for total sizes up to 2 KiB, and a size -> head map, one entry per distinct size, for the rest. Listing a hole is one store to the word beside the header the rebuild walk has just read. A take pops its size's head.

  The hole header is unchanged (`obj_type == 0`, `size` intact), so walkers, address lookups and verifiers see the same dead hole as before; only the first payload word changes. A hole smaller than header + link (16 bytes) is not listed, since its link would overwrite the next object's header.

  Unlinking is where the shape costs something. A chain runs through every block that holds one of its holes, so a block reset must unlink that block's holes before its bytes are reused or released, and unlinking walks every chain by chasing links through the heap rather than scanning a dense vector. `ArenaBlock` therefore gains an `old_free_holes` bit, maintained by the rebuild walk: set on each old block where it listed a hole, cleared on every other block. The three old-block reset sites (`old_arena_reclaim_dead_blocks`, `old_arena_reclaim_selected_dead_blocks`, `OldArenaReclaimDeadBlocksState::process_block`) filter only a block whose bit is set. A full sweep rebuilds the list from its live blocks before its block cleanup, so every block that cleanup recycles has its bit clear, and full reclaims never walk the chains. Only a targeted (defrag) reclaim of a block that held listed holes pays for a walk. The defrag-time take (`old_free_take_exact` with excluded pages) stops at the first hole off the excluded pages instead of walking the rest of the chain.

  Measured on a 4-core Linux VM, with the same release settings for both arms. The runtime-level churn fixture allocates 700k old objects, keeps every 64th live, and refills half the holes between sweeps, so about 24 MB of holes are listed per cycle. Allocation counts come from the `alloc-census` allocator wrapper and are exact:

  | per sweep cycle | before | after |
  |---|---:|---:|
  | list rebuild: allocations / bytes | 48 / 6.29 MB | 0 / 0 |
  | whole old-gen sweep: allocations / bytes | 4,726 / 6.97 MB | 4,678 / 0.68 MB |
  | hole reuse (refill): allocations | 9,260 | 9,260 |

  The rebuild was faster in both timing runs (median of 15 cycles: 5.4 → 4.7 ms and 7.5 → 6.2 ms). Whole-sweep and refill times moved within this machine's run-to-run noise. On the `12_large_live_set` ratchet probe, whose final full GC lists 37.6 MB of holes, peak RSS drops from 156.0 to 146.1 MB (median of 15 interleaved runs; a second run of 7 gave the same figures). Wall time and full-GC sweep time on probes 02, 06, 12 and 14 showed no difference distinguishable from noise.

  Tests (`gc/tests/old_free_intrusive.rs`, all driving real sweeps):
  - reuse and chain consistency across two sweeps, for both head kinds;
  - a reset block's holes unlinked from a chain shared with a surviving block, at all four reclaim entry points;
  - a defrag-time take that skips and keeps holes on excluded pages;
  - a header-only hole left unlisted with its neighbour's header intact.

  Each test was sabotage-checked: the mechanism it pins was broken and the test went red at the intended assertion, then the code was restored. The sabotages were: a take that does not advance the head; a full reclaim that ignores the bit; the selected and stepwise reclaims ignoring it (red for exactly those three entry points); a rebuild that never sets the bit; the minimum-hole guard reverted to the header size; a defrag take that skips the page check; and a defrag take that truncates the chain.
