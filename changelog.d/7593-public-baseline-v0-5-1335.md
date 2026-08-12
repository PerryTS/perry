### Benchmarks

- **The public Node/Bun baseline is regenerated and the gate that publishes it
  can pass again (#7593).** `lint` had been red on `main` and on every open PR
  for **two independent reasons**, only one of which was known.

  **The artifact was stale, and the gate was right to say so.** Its
  `source_fingerprint` covers `Cargo.toml`, and #7302 changed
  `panic = "unwind"` → `panic = "abort"` in the `release` and `dist` profiles —
  a genuinely benchmark-relevant change. (The tempting hypothesis, that the
  per-merge version bump invalidates it every time, is wrong: `_CARGO_VERSION_RE`
  already normalises the version line.) Regenerated on the pinned quiet M1 Max —
  the host the artifact's own `host.cpu` names — with pinned `node v22.23.1` /
  `bun 1.3.14` / `zig 0.15.2`, all five `wait_for_quiet` gates passed, at
  `2ba59501b` (v0.5.1335).

  **And `public_baseline.py check` could never pass.** #6736 rewrote `README.md`
  as a marketing landing page and deleted the `<!-- public-node-bun:start/end -->`
  markers, so `_replace_block` raised `README generated markers are missing` on
  every invocation regardless of the artifact — no amount of regenerating would
  have fixed it. Worth naming as a pattern rather than a typo: **a gate whose
  subject has been deleted still runs, still reports, and can never go green**,
  so it stops being a gate and becomes a standing reason to bypass. Same family
  as #7582.

  The markers are restored under the sentence that promises them — *"We publish
  everything, including the workloads where V8's JIT still beats us — no
  cherry-picked table can survive an open harness"* — inside a collapsed
  `<details>`, so the landing page keeps the shape #6736 chose while the
  published numbers are again **derived from the artifact** rather than
  hand-maintained. The generated table is what makes that sentence true: it
  carries `prime_sieve` (30 ms vs 6) and `matrix_multiply` (87 ms vs 34) as
  losses.

  **What the run measured**, output verified against the Bun reference 20/20 on
  every row: `image_convolution` **302.5 ms**, 3.1× faster than bun (947.2) and
  faster than **Rust** (429.8), within 8% of Zig (279.8); and `json_pipeline` at
  100 records **77.0 ms**, ahead of bun (90.8) and node (128.1), within 5% of
  Rust and Zig, at **8.5 MB RSS against bun's 30.3**.

  The same JSON workload at **500,000 records is 60,358 ms against bun's 618 ms
  — 97.6×**, filed as **#7592**. It is a scaling cliff rather than a constant
  factor: across the fixture increase Perry grows 784× where bun grows 6.8×,
  Rust 8.9× and Zig 12.0×. Output correct 20/20, so purely performance. That row
  is published by this change, which is the point of the mechanism.
