# Run the Perry doc-example test harness on Windows.
#
# Mirror of scripts/run_doc_tests.sh. Used by the Windows CI runner and
# Windows developers. Forwards any extra args through to the harness
# (e.g. --filter, --verbose, --bless, --filter-exclude).

$ErrorActionPreference = 'Stop'

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot  = Resolve-Path (Join-Path $ScriptDir '..')

Set-Location $RepoRoot

# Build perry + UI backend + harness in release mode. Skipped transparently
# if already built.
cargo build --release `
    -p perry `
    -p perry-runtime `
    -p perry-stdlib `
    -p perry-runtime-static `
    -p perry-stdlib-static `
    -p perry-ui-windows `
    -p perry-doc-tests
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# Host doc-tests can reuse the full prebuilt runtime/stdlib archives above.
# This mirrors run_doc_tests.sh and avoids a feature-specialized Cargo rebuild
# for each of ~90 examples. Cross-compile runs must leave auto-optimization on
# because it is what produces target-specific archives.
if ($Args -notcontains '--xcompile-only') {
    $env:PERRY_NO_AUTO_OPTIMIZE = '1'
}

$ReportDir = Join-Path $RepoRoot 'docs\examples\_reports'
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null
$ReportJson = Join-Path $ReportDir 'latest.json'

# Forward remaining positional args through to the harness.
cargo run --release --quiet -p perry-doc-tests -- `
    --json $ReportJson `
    @Args
exit $LASTEXITCODE
