**Fixed** the compact stack-map rewrite refused every module on aarch64-ELF at
`-O3`, so `native-roots-rs4gc (ubuntu-24.04-arm)` could never pass and statepoints
— on by default for aarch64 — could not compile on Linux arm64.

`gc_map`'s parser walks the stack-map block directive by directive, because the
block is a byte stream decoded by structural offset: one unmodelled directive
that *does* emit bytes shifts everything after it, so anything unrecognised is a
hard error rather than a skip. That is the right default and it is why this
surfaced as a refusal instead of a corrupt decode.

What it did not model is the GNU-as **symbol assignment**, `sym = expr` — the
bare spelling of `.set`, which emits zero bytes and has no leading directive. The
dispatch therefore reported the *symbol* as an unrecognised directive:

```
line 5114: unrecognised directive `perry_class_keys__…__AnonShape_…`
           inside the stack-map block
```

Only `-O3` emits it (the optimiser materialises absolute-symbol aliases such as
`perry_null_guard_zero = 0` and `.Lperry_ic_8 = .Ltmp3-4`), and only on ELF —
Mach-O's asm printer does not use this spelling, so every macOS arm stayed green.

The guard tests for "not a directive this module already models" rather than "no
leading dot", because ELF local labels legitimately start with `.L` and appear on
the left of these assignments. Expression operators (`==`, `!=`, `>=`, `<=`) are
excluded so an `.if` is never mistaken for one.

Reproduced locally without a Linux host by retargeting a traced module to
`aarch64-unknown-linux-gnu`, running `rewrite-statepoints-for-gc`, and emitting
with `llc -O3 -mattr=+jsconv,+v8.3a`; the real assembly parses at `-O2` and
refused at `-O3` exactly as CI reported, and parses at both with the fix.

**Also fixed** the assembler was invoked without the CPU the code generator was
given, so aarch64-linux failed a second time once the parse was fixed:

```
error: instruction requires: sve or sme
        mov     z1.d, #0x7fffffffffffffff
```

Perry compiles with `-mcpu=native`. On a host whose CPU has SVE — Graviton, and
any aarch64 server part — LLVM emits SVE instructions. `compact_and_assemble`
then handed that text to clang with no `-mcpu` at all, so the assembler applied
the portable baseline and rejected what the generator had just produced.

The two invocations describe the same machine and now say so: the codegen argv's
`-mcpu=`/`-march=`/`-mtune=` flags are forwarded to the assembler. Optimisation
and output flags are deliberately not, since they mean nothing to an assembler
and forwarding them wholesale would be a second way for the two to disagree.

This is invisible on the macOS arms, whose runner CPUs have no SVE.
