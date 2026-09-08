# Claude Code bundle parity

The `cc-parity` workflow compiles the standalone Claude Code **2.1.112** npm
bundle with Perry and compares native `--help` and `--version` stdout with
checked-in Node output. It covers the bundle-scale regressions described in
[#9346](https://github.com/PerryTS/perry/issues/9346).

## Opt in

Apply **`run-cc-parity`** to a PR changing compiler/runtime crates, build inputs,
or the gate itself. The workflow also supports manual dispatch. Unlabelled PRs
skip every job; labelled documentation-only PRs skip the expensive job. A new
commit supersedes the previous run on the same PR.

This starts as a **non-required** check. Adding it to branch protection is a
separate maintainer decision after successful hosted runs. It has no push,
schedule, or release-tag trigger and is independent of `run-extended-tests`.

The expensive job uses one `macos-15-intel` runner. Its
[14 GB RAM allocation](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
provides more headroom for bundle IR construction than the 7 GB ARM runner.
The job removes unused simulator images and disables Cargo incremental artifacts
to leave disk space for LLVM and the native archives. The issue estimated 25–40
minutes for bundle compilation; local validation took **57 minutes 25 seconds**
on macOS arm64 with five LLVM workers. The hosted Intel run with four workers
remains to be measured. Allow additional time for toolchain setup, especially on
a cold cache. The job has a 90-minute cap, compilation a 75-minute cap, and each
CLI invocation a 60-second cap. Timings are recorded for diagnosis, not compared
with a performance threshold.

## What the check proves

`tests/cc-parity/manifest.json` pins the npm tarball and extracted `package/cli.js`
by both size and SHA-256. Only that regular file is extracted; no package install
hooks run. The compiler is built first, then the runtime, stdlib, Wasm host, and
all native extension archives are built together with `perry-runtime/wasm-host`.
This avoids stale runtime copies in extension archives (#6303). Compilation uses
`--no-auto-optimize --no-cache --enable-wasm-runtime`, with four LLVM workers
(`PERRY_CODEGEN_UNIT_JOBS=4`) to use the Intel runner's four cores within its
memory budget.

The runtime arm requires a native Mach-O executable. Each invocation gets its own
temporary HOME, XDG directories, working directory, and TMPDIR, with a small
environment allowlist and no inherited credentials or compiler tuning knobs.
macOS `sandbox-exec` denies network access; the gate fails if that sandbox is
unavailable. The harness tests include an attempted connection to prove the
network restriction is active.

Both commands must exit zero before their deadlines and produce exactly the
golden bytes: **9,175 bytes** for help and **22 bytes** for version. A crash,
timeout, empty output, or one-byte difference fails. The manifest also pins the
goldens themselves, so changing a golden without updating its identity fails.

Downloading the bundle, LLVM, and Rust dependencies requires network access
during setup. The two CLI executions are offline and use no Node installation
or API key. The artifact contains source identity, build/compile logs, actual
stdout/stderr, and JSON results; it excludes the downloaded bundle and executable.

## Run locally on macOS

From the repository root, with LLVM 22 and the pinned Rust toolchain available:

```bash
export LLVM_SYS_221_PREFIX="$(brew --prefix llvm@22)"
export CARGO_BUILD_JOBS=4
cc_work="$(mktemp -d)"
python3 -m unittest discover -s tests -p test_cc_parity_gate.py -v
python3 scripts/cc_parity_gate.py prepare --work-dir "$cc_work"
python3 scripts/cc_parity_gate.py build --work-dir "$cc_work"
python3 scripts/cc_parity_gate.py compile --timeout 4500 --work-dir "$cc_work" --perry "$PWD/target/perry-dev/perry"
python3 scripts/cc_parity_gate.py check --work-dir "$cc_work"
```

If using `CARGO_TARGET_DIR`, pass the compiler in that directory instead. A local
tarball can be supplied to `prepare --archive <path>`; the same hashes are still
required. Inspect `$cc_work/logs/` for output differences and failure details.
Never run the bundle using your regular HOME: Claude can write its configuration
even on startup paths.

## Refresh the pin and oracle deliberately

2.1.112 is a standalone `cli.js` release. A newer package may have a different
distribution shape; confirm it still supplies the full bundle before changing
the pin. Update the manifest's version, URL, archive identity, and bundle identity
from the exact public npm tarball, then run `prepare` again.

The recorded reference used Node **v26.5.1** on macOS arm64. To verify that oracle
with the same scratch environment and network sandbox:

```bash
python3 scripts/cc_parity_gate.py check --work-dir "$cc_work" --node "$(command -v node)"
```

This writes `logs/node-help.stdout`, `logs/node-version.stdout`, and
`logs/node-parity.json`. A deliberate version refresh may fail the old golden
comparison; inspect both command results, require zero exit codes and no timeout,
and review the output changes before copying those two stdout files into
`tests/cc-parity/`. Update their byte counts and SHA-256 values and the oracle
provenance in the manifest. Rerun the Node check, harness tests, native compilation,
and native check. Commit the manifest and goldens together; never accept output
from a failing native executable as the new oracle.
