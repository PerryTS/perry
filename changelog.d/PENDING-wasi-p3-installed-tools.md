Add the WASI 0.3 runtime bundle target and an isolated installed-tool check for
public P3 plugin generation, composition, typed native loading and commands.
Require actual rejection of missing guest runtimes and native component engines
before installing them, so source-checkout archives cannot silently satisfy the
package checks.
