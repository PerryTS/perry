# node:vm parity fixtures

This directory covers the default Node `node:vm` surface that is visible
without `--experimental-vm-modules`: import and require shapes, callable export
metadata, `vm.constants`, `process.getBuiltinModule("vm")`, `vm.isContext({})`,
and the narrowed deterministic execution subset for `Script`, context-backed
sandbox mutation/isolation, and `compileFunction`.

Intentionally open leaves:

- VM module classes and evaluation: #3132, #3133
- measureMemory: #3284
- Script sourceMapURL metadata: #3321
- SourceTextModule module request helpers: #3322
- SourceTextModule cached data: #3323
