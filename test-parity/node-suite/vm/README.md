# node:vm parity fixtures

This directory covers the default Node `node:vm` surface that is visible
without `--experimental-vm-modules`: import and require shapes, callable export
metadata, `vm.constants`, `process.getBuiltinModule("vm")`, and
`vm.isContext({})`.

The `modules/` fixtures opt into Node's `--experimental-vm-modules` flag and
Perry's matching `PERRY_EXPERIMENTAL_VM_MODULES=1` gate to cover deterministic
`SourceTextModule`/`SyntheticModule` lifecycle behavior separately from the
default surface.

Intentionally open leaves:

- Script compilation/execution: #3127
- Contextified sandbox execution: #3128
- compileFunction: #3130
- Full VM module parsing/evaluation beyond deterministic lifecycle fixtures:
  #3132, #3133
- vm.constants deeper context-loader behavior: #3283
- measureMemory: #3284
- Script sourceMapURL metadata: #3321
- SourceTextModule cached data: #3323
