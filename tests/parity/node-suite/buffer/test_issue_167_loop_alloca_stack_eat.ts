// Regression test for issue #167.
//
// Before the fix, two `alloca [N x double]` sites in
// `crates/perry-codegen/src/lower_call.rs` (the `js_native_call_method`
// dispatch for dynamic methods + the class-dispatch fallback) emitted into
// the *current* basic block. Inside a loop body, LLVM lowers a non-entry
// alloca as a runtime `sub %rsp, N` with no matching restore — every
// iteration permanently shrinks the stack, SIGSEGVing around 250k–300k
// iterations on macOS arm64 (8 MB stack).
//
// The repro from the issue: a 300k-iteration loop calling
// `buf.readInt32BE(i*4)`. With the fix the alloca is hoisted to the
// function entry block, so the loop body is alloca-free and the stack stays
// flat regardless of N. We use 500_000 here so the test fails decisively
// pre-fix (runs out well before completion) and runs in <100 ms post-fix.

const N = 500_000;
const buf = Buffer.alloc(N * 4);
for (let i = 0; i < N; i++) buf.writeInt32BE(i * 37, i * 4);
let sum = 0;
for (let i = 0; i < N; i++) sum += buf.readInt32BE(i * 4);
console.log("sum=" + (sum & 0xFFFF));
