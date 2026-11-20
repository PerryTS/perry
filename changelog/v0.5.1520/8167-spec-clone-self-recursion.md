### Performance

- A specialized `$spec_*` entry can now re-enter **itself**. The only raw-`i32`
  argument shapes a call site could prove were an `i32` literal and a bare
  `LocalGet` of an integer local, and a recursive call's argument is almost
  always derived (`fib(n - 1)`), so every recursive edge inside a clone
  targeted the generic public symbol. The clone therefore ran exactly once per
  top-level call and the whole recursion paid dynamic dispatch — on `fib(40)`
  that was one fast call out of ~331 million. Compiling
  `function fib(n: number): number { return n < 2 ? n : fib(n-1) + fib(n-2); }`
  now retires **4.68 G instructions instead of 227.85 G** (48.7x; 13.76 s →
  0.82 s user, same host, compiler-only A/B against `48935af78`).

  The new proof composes the leaf fact the entry already owns. A parameter the
  entry binds as a raw LLVM `i32` is finite, integral and not `-0`, and integer
  literals and `+`/`-` over such leaves preserve all three while magnitudes stay
  under 2^53. The one remaining obligation from the slot contract
  (`js_typed_i32_arg_guard` in `perry-runtime/src/native_abi.rs`) is 32-bit
  containment, and that is precisely where assuming "it is an integer, ship it"
  would be wrong: `n - 1` for an i32 `n` is `[-2^31 - 1, 2^31 - 2]`, one value
  wider than the slot. A window inside the slot is called directly, a window
  that merely overlaps takes one range test with the permanent boxed entry as
  the cold arm, and a window with no overlap keeps the boxed path with no
  diamond emitted.

  Multiplication is deliberately outside the derivation, and that is measured
  rather than argued: `n * 0` with `n < 0` is `-0`, which the guard rejects on
  purpose because the raw slot has no `-0` to round-trip through. Admitting
  `Mul` makes
  `function probe(n: number): number { if (n === -5) return probe(n * 0); return 1 / n; }`
  print `Infinity` where node `v26.5.1` prints `-Infinity`.

  Mutual recursion (`f → g → f`) and higher-order calls are out of scope: the
  leaf fact does not cross a function boundary, and nothing here changes that.
