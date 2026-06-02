# node:vm parity fixtures

This directory covers the default Node `node:vm` surface that is visible
without `--experimental-vm-modules`: import and require shapes, callable export
metadata, `vm.constants`, `process.getBuiltinModule("vm")`, and
`vm.isContext({})`. `measureMemory` is covered for Node-shaped eager summary
and detailed results, option validation, and experimental-warning delivery;
the exact byte estimates remain Perry-specific.

Intentionally open leaves:

- Script compilation/execution: #3127
- Contextified sandbox execution: #3128
- compileFunction: #3130
- VM module classes and evaluation: #3132, #3133
- vm.constants deeper context-loader behavior: #3283
- Script sourceMapURL metadata: #3321
- SourceTextModule module request helpers: #3322
- SourceTextModule cached data: #3323
