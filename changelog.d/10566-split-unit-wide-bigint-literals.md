**BigInt literals wider than 64 bits no longer change value in a split module.** A module compiled as more than one
codegen unit truncated every BigInt literal that needs more than 64 bits: `2n ** 70n === 1180591620717411303424n` was
`false`, the literal printed `0`, and a 97-bit negative literal became a different 64-bit number. Single-unit builds
were correct, so the gap/parity corpus (all single-unit) could not see it, while large real modules split on their own
— any split module holding a >64-bit BigInt literal (curve primes, group orders, field moduli) silently computed with
the wrong constant (#10545).

Root cause: `crates/perry-codegen/src/dialect/types.rs`'s constant reader built every integer operand with
`IntType::const_int(v as u64, v < 0)`, which takes a single 64-bit word, so an `i128` operand kept only its low word.
Every BigInt literal that fits in `i128` lowers to exactly those operands (`NativeRep::SmallBigInt` →
`trunc i128 C to i64` / `ashr i128 C, 64`), and the in-process dialect reader is the default only for split modules —
LLVM's own assembler, used on the single-unit and external-clang paths, reads the literal at arbitrary precision and
then extends or truncates it to the operand width. Same class as #8228/#8241: a form the closed-set reader gets wrong
is invisible to every per-PR job.

Fix: widths above 64 bits now build both two's-complement words with `const_int_arbitrary_precision`, the non-negative
case parses as `u128` so LLVM's unsigned full-width spelling is accepted, and an integer type wider than `i128` is
refused loudly instead of silently truncated.

Validation: a split-unit integration test (`crates/perry/tests/issue_10545_split_unit_wide_bigint_literals.rs`, which
also asserts both `unitN.native.ll` files exist so it cannot pass without the reader), a reader unit test that builds
every constant operand form codegen emits — through the typed and the line paths — and compares each against LLVM's
own parse of the same text (only the `i128` rows diverged), a unit test that drives the real emitter and checks the
words handed to `js_bigint_from_i128_parts`, and a gap test. Each fails on the parent commit. Gap suite unchanged (820
tests, same six known non-passing); `perry-codegen` tests green; compile-time A/B of the compiler built with and
without the change: −0.07 % instructions on a module with no wide constants (byte-identical objects) and +0.005 % on a
`@noble/curves` program.
