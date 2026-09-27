Bind unshadowed require values in compiled TypeScript packages to the defining
module's filename. Literal .cjs requires and first-class require aliases now
resolve relative to their source module instead of the process working directory.
This completes the remaining relative-path case from #10436 after the earlier
static JSON import fix.

Add lowering coverage for distinct package filenames and a native integration
test comparing Node across package, project, and decoy working directories,
including CommonJS/JSON values, require.resolve/cache, and a shadowed require.
