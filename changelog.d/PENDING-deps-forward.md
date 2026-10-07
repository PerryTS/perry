Migrate the compiler and native TypeScript extension to one compatible SWC
dependency family: swc_common 26, swc_ecma_ast 29, swc_ecma_parser 45,
swc_ecma_codegen 32, swc_ecma_visit 29 and swc_ecma_transforms_base 49.
The extension now inherits the workspace versions. This removes the duplicate
SWC AST/visitor/transform stacks; it adds no caches, latches or side tables.

Port function and arrow bodies to `FunctionBody` / `ArrowFunctionBody`, and
object accessors to their backing `Function`, runtime-evaluated arrow bodies,
and JSX text to the new WTF-8 accessors. Keep the existing lowering,
strict-mode checks, closure scans and CommonJS require-shadowing diagnostics.
TypeScript's type-only `this` parameter is now separate from runtime parameters.

LLVM stays on llvm-sys 221.0.1 and LLVM 22. Migrating to llvm-sys 231 requires:

- LLVM 23.1 development libraries, headers and a matching `llvm-config`,
  configured with `LLVM_SYS_231_PREFIX`. The validation host has LLVM 21/22.
  Provision matching LLVM 23 on Linux, macOS and Windows build/release hosts;
  update the LLVM setup action, Windows import-library build script and LLVM
  version checks. Rebuild all compiler/runtime archives and validate native
  roots, stack maps, exception unwinding and performance on supported targets.
- A coordinated Inkwell upgrade/port with LLVM 23 support. Current Unix
  Inkwell 0.10.0 and Windows 0.9.0 select `llvm22-1`; changing Perry's direct
  llvm-sys dependency alone leaves Inkwell using incompatible LLVM 22 bindings.
- The [LLVM 23 changelog](https://releases.llvm.org/23.1.0/docs/ReleaseNotes.html#changes-to-the-c-api)
  replaces `LLVMBr` with `LLVMUncondBr` / `LLVMCondBr` and changes conditional
  branch operand order. Audit Inkwell's branch dispatch and any raw successor
  operand indexing; Perry's native-home CFG walk already uses `LLVMGetSuccessor`.
  The [231 bindings](https://docs.rs/crate/llvm-sys/231.0.0/source/src/core.rs)
  deprecate `LLVMIsConditional` / `LLVMIsABranchInst` in favor of the new branch
  predicates, correct `LLVMGetSyncScopeID` to return `c_uint`, and add byte types,
  byte constants and first-class denormal attributes. These functions are not
  directly called by Perry. Debug-location setting is renamed to
  `LLVMSetInstDebugLocation`; `LLVMAddMetadataToInst` becomes its deprecated
  alias. The changelog also changes printed floating-point IR literals to new
  formats, so audit Perry's NaN-boxed hexadecimal constants and IR assertions.

Validation results are recorded below when both builds finish. Project/release
versions and llvm-sys are unchanged.
