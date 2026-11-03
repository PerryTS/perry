### Fixed

- **Integer arithmetic in a local no longer evaluates past double precision (#7232).**
  `(x * 1103515245 + 12345) & 0x7fffffff` — an LCG step — printed `654583775`
  where Node prints `654583808`. The i32-native fast path
  (`crates/perry-codegen/src/expr/i32_fast_path.rs`) evaluated the whole chain
  in exact two's-complement `mul/add i32`; ECMAScript evaluates it in doubles,
  rounding at every operator. The ~2^61 product is past 2^53, so the double had
  already discarded the low bits the exact chain still carried, and the mask
  read them straight back. Wrong straight-line and loop-carried, correct only
  through a function boundary (where the intermediate gets NaN-boxed and
  therefore rounded) — so PRNG seeds, hash mixing, checksum accumulators and ID
  arithmetic diverged silently, with no throw and no warning.

  The old admission rule required only that every integer *literal* fit in i32,
  which is neither necessary (`Math.imul` is defined as an exact low-32
  multiply) nor sufficient: `1103515245` fits, and its product with an
  i32-range local does not. It is replaced by a magnitude bound carried through
  the whole chain and capped at 2^53, the largest integer a double represents
  exactly: **below the cap the JS double *is* the exact integer and
  `low32(exact) == ToInt32(double)`; above it the two models are different
  numbers**, so the chain now falls onto the f64 path whose `fmul`/`fadd` round
  where the spec says to. The cap applies to `Add`/`Sub` as well as `Mul` —
  two ceiling-width products sum to 2^54 — and both emitters of the chain
  consult the same bound, so the gate and the last-resort arithmetic arm cannot
  drift apart.

  The bound is measured rather than assumed, so correct code keeps its fast
  path: an integer literal contributes its own bit width (`h * 31 + c` is 37
  bits, not 64), `x & m` with a non-negative literal mask lands in `[0, m]`,
  `x >> k` / `x >>> k` by a literal count drop `k` bits, a `const` bound to a
  numeric literal contributes *its* width (which is what keeps
  `buf[y * WIDTH + x]` exact), and `Math.imul` is exempt entirely. Across all
  30 programs in `benchmarks/suite/`, 29 emit byte-identical LLVM IR to before;
  the exception is `11_prime_sieve`, whose `for (let j = i * i; …)` preheader
  is genuinely unbounded and now rounds.

  Covered by `test-files/test_gap_7232_i32_chain_double_rounding.ts` (the
  issue's three shapes, every ToInt32-shaped consumer, the 2^53 boundary from
  both sides, and the chains that must stay exact) and by fourteen unit tests in
  `crates/perry-codegen/src/expr/i32_fast_path/bits_tests.rs` that are
  sabotage-checked in both directions.
