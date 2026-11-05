Make the GC-root tests and dominance checker recognize inline class and empty-array births. Follow the fast/slow instance phi, retain per-value rooting and reload assertions, and check branch-local allocation origins through the dominating join. Audit the emitted helper definitions and classify only the slow birth calls as collecting; fast call sites retain their GC-leaf classification. Add negative controls for missing instance coverage.

The archive-symbol cross-check also includes the default stdlib's bundled zlib provider, keeping its 47 moved exports visible to the same allocation and poll audits.

A CLI regression test for the target-wasi build requires real WASI IR/object output and rejects any Cargo invocation during compile-only emission. The full shadow corpus must use that feature: without it the short wasi target name falls back to native codegen.

The enabled WASI corpus exposed two invalid bare-i32 runtime ABI tokens. Use the generated unsigned/signed tokens for the corresponding u32/i32 runtime parameters so the WASI adapter can parse its table. Native codegen is unchanged.

Scan whole SSA tokens once per operand lookup instead of compiling a regex per candidate register. Skip tokenization when a small candidate set has no literal spelling in the text; possible prefix hits still require whole-token validation. This removes regex-cache churn and unnecessary scanning of large live bundles while preserving the same register boundaries and gate assertions.

The expanded native gate found array elements retained in registers across the collecting inline allocator and a reduceRight callback retained across its initial empty-array birth. Keep literal/rest elements in their existing rooted group until allocation finishes, then re-read them for initialization. Protect reduce operands through initial-value evaluation and validation, and re-read each below its last collecting step. Native regression tests cover both lifetimes on Linux x86-64 and macOS ARM64 IR.
