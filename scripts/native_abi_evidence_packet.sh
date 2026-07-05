#!/usr/bin/env bash
# Build a PR-ready native ABI evidence packet for the current checkout.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNS=5
OUT=""
PERRY_ARG="${PERRY:-${PERRY_BIN:-}}"
GATE=0

usage() {
  cat <<'EOF'
Usage: scripts/native_abi_evidence_packet.sh [options]

Options:
  --runs N       Benchmark samples for packet workloads (default: 5)
  --out PATH     Output root (default: tmp/native-abi-evidence-<utc>)
  --perry PATH   Perry binary to use instead of resolving/building one
  --gate         Fail on missing or failing required evidence
  -h, --help     Show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --runs) RUNS="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --perry) PERRY_ARG="$2"; shift 2 ;;
    --gate) GATE=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if ! [[ "$RUNS" =~ ^[0-9]+$ ]] || [[ "$RUNS" -lt 1 ]]; then
  echo "--runs must be a positive integer" >&2
  exit 2
fi

if [[ "$GATE" -eq 1 && "$RUNS" -lt 5 ]]; then
  echo "--gate requires --runs >= 5 so p95 packet timing evidence is meaningful" >&2
  exit 2
fi

if [[ -z "$OUT" ]]; then
  OUT="tmp/native-abi-evidence-$(date -u +%Y%m%dT%H%M%SZ)"
fi

cd "$ROOT"

ORIGINAL_RUSTC_WRAPPER="${RUSTC_WRAPPER:-}"
ORIGINAL_RUSTFLAGS="${RUSTFLAGS:-}"
CARGO_CONFIG_RUSTC_WRAPPER=""
for cargo_config in "${CARGO_HOME:-$HOME/.cargo}/config.toml" "$ROOT/.cargo/config.toml"; do
  if [[ -f "$cargo_config" ]]; then
    cargo_config_text="$(tr -d '[:space:]' < "$cargo_config")"
    if [[ "$cargo_config_text" == *"rustc-wrapper="* ]]; then
      CARGO_CONFIG_RUSTC_WRAPPER="configured"
      break
    fi
  fi
done
SCRUBBED_RUSTC_WRAPPER=0
if [[ "${PERRY_EVIDENCE_KEEP_RUSTC_WRAPPER:-0}" != "1" && ( -n "$ORIGINAL_RUSTC_WRAPPER" || -n "$CARGO_CONFIG_RUSTC_WRAPPER" ) ]]; then
  export RUSTC_WRAPPER=""
  SCRUBBED_RUSTC_WRAPPER=1
fi
if [[ -n "${PERRY_EVIDENCE_RUSTFLAGS:-}" ]]; then
  export RUSTFLAGS="$PERRY_EVIDENCE_RUSTFLAGS"
elif [[ -z "$ORIGINAL_RUSTFLAGS" ]]; then
  export RUSTFLAGS="-Awarnings"
fi

PYTHON_BIN="${PYTHON:-}"
if [[ -z "$PYTHON_BIN" ]]; then
  if command -v python3.11 >/dev/null 2>&1; then
    PYTHON_BIN="$(command -v python3.11)"
  else
    PYTHON_BIN="$(command -v python3)"
  fi
fi

OUT_ABS="$("$PYTHON_BIN" - "$ROOT" "$OUT" <<'PY'
import os
import sys
root, out = sys.argv[1], sys.argv[2]
if not os.path.isabs(out):
    out = os.path.join(root, out)
print(os.path.abspath(out))
PY
)"
OUT_REL="$("$PYTHON_BIN" - "$ROOT" "$OUT_ABS" <<'PY'
import os
import sys
root, out = map(os.path.abspath, sys.argv[1:3])
rel = os.path.relpath(out, root)
if rel.startswith(".."):
    raise SystemExit(1)
print(rel)
PY
)" || {
  echo "output path must be inside the repository: $OUT_ABS" >&2
  exit 2
}

if ! git check-ignore -q -- "$OUT_REL"; then
  echo "output path must be ignored by git: $OUT_REL" >&2
  exit 2
fi

if [[ -n "$(git ls-files -- "$OUT_REL" "$OUT_REL/**")" ]]; then
  echo "output path contains tracked files; choose a fresh ignored path: $OUT_REL" >&2
  exit 2
fi

mkdir -p "$OUT_ABS/logs"
METADATA="$OUT_ABS/metadata.json"

write_metadata() {
  "$PYTHON_BIN" - "$METADATA" "$RUNS" "$GATE" "$PYTHON_BIN" "$ORIGINAL_RUSTC_WRAPPER" "$CARGO_CONFIG_RUSTC_WRAPPER" "$SCRUBBED_RUSTC_WRAPPER" "${RUSTC_WRAPPER:-}" "$ORIGINAL_RUSTFLAGS" "${RUSTFLAGS:-}" <<'PY'
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

path = Path(sys.argv[1])
existing = {}
if path.exists():
    existing = json.loads(path.read_text(encoding="utf-8"))
existing.update({
    "schema_version": 1,
    "generated_at": existing.get("generated_at") or datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "runs": int(sys.argv[2]),
    "gate": sys.argv[3] == "1",
    "python": sys.argv[4],
    "commands": existing.get("commands", {}),
    "environment": {
        **existing.get("environment", {}),
        "rustc_wrapper_original": sys.argv[5],
        "rustc_wrapper_config": sys.argv[6],
        "rustc_wrapper_scrubbed": sys.argv[7] == "1",
        "rustc_wrapper_effective": sys.argv[8],
        "rustflags_original": sys.argv[9],
        "rustflags_effective": sys.argv[10],
        "keep_rustc_wrapper": os.environ.get("PERRY_EVIDENCE_KEEP_RUSTC_WRAPPER") == "1",
        "rustflags_override": os.environ.get("PERRY_EVIDENCE_RUSTFLAGS", ""),
    },
    "tool_versions": existing.get("tool_versions", {}),
})
path.write_text(json.dumps(existing, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

record_command() {
  local label="$1"
  local name="$2"
  local status="$3"
  local exit_code="$4"
  local log_path="${5:-}"
  local reason="${6:-}"
  "$PYTHON_BIN" - "$METADATA" "$label" "$name" "$status" "$exit_code" "$log_path" "$reason" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

path = Path(sys.argv[1])
label, name, status, exit_code, log_path, reason = sys.argv[2:8]
data = json.loads(path.read_text(encoding="utf-8"))
entry = {
    "status": status,
    "exit_code": int(exit_code),
    "finished_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
}
if log_path:
    entry["log"] = log_path
if reason:
    entry["reason"] = reason
data.setdefault("commands", {}).setdefault(label, {})[name] = entry
path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

capture_tool_versions() {
  "$PYTHON_BIN" - "$METADATA" "$ROOT" <<'PY'
import json
import platform
import subprocess
import sys
from pathlib import Path

metadata = Path(sys.argv[1])
root = Path(sys.argv[2])

def run(cmd):
    try:
        completed = subprocess.run(cmd, cwd=root, text=True, capture_output=True, timeout=20)
    except Exception as exc:
        return {"available": False, "error": str(exc)}
    return {
        "available": completed.returncode == 0,
        "exit_code": completed.returncode,
        "stdout": completed.stdout.strip().splitlines()[:3],
        "stderr": completed.stderr.strip().splitlines()[:3],
    }

data = json.loads(metadata.read_text(encoding="utf-8"))
data["tool_versions"] = {
    "platform": platform.platform(),
    "python": sys.version.split()[0],
    "git": run(["git", "--version"]),
    "cargo": run(["cargo", "--version"]),
    "rustc": run(["rustc", "--version"]),
    "clang": run(["clang", "--version"]),
    "cc": run(["cc", "--version"]),
    "ar": run(["ar", "--version"]),
}
metadata.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

run_logged() {
  local label="$1"
  local name="$2"
  local log="$3"
  shift 3
  mkdir -p "$(dirname "$log")"
  echo "=== $label: $name ==="
  set +e
  (
    cd "$ROOT"
    "$@"
  ) >"$log" 2>&1
  local code=$?
  set -e
  local status="pass"
  if [[ "$code" -ne 0 ]]; then
    status="fail"
  fi
  record_command "$label" "$name" "$status" "$code" "$log" ""
  echo "  $status (exit=$code, log=$log)"
  return 0
}

resolve_perry() {
  if [[ -n "$PERRY_ARG" ]]; then
    case "$PERRY_ARG" in
      /*) printf '%s\n' "$PERRY_ARG" ;;
      *) printf '%s\n' "$ROOT/$PERRY_ARG" ;;
    esac
    return
  fi
  if [[ -x "$ROOT/target/release/perry" ]]; then
    printf '%s\n' "$ROOT/target/release/perry"
    return
  fi
  if [[ -x "$ROOT/target/debug/perry" ]]; then
    printf '%s\n' "$ROOT/target/debug/perry"
    return
  fi
  printf '%s\n' "$ROOT/target/debug/perry"
}

resolve_runtime_archive() {
  local perry_bin="$1"
  local dir
  dir="$(dirname "$perry_bin")"
  for candidate in "$dir/libperry_runtime.a" "$dir/perry_runtime.lib"; do
    if [[ -f "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return
    fi
  done
  printf '%s\n' "$dir/libperry_runtime.a"
}

snapshot_tool_artifacts() {
  local perry_bin="$1"
  local runtime_archive="$2"
  local tools_dir="$OUT_ABS/tools"
  mkdir -p "$tools_dir"

  if [[ -x "$perry_bin" ]]; then
    cp "$perry_bin" "$tools_dir/perry"
    chmod +x "$tools_dir/perry"
    PERRY_BIN_RESOLVED="$tools_dir/perry"
  fi

  if [[ -f "$runtime_archive" ]]; then
    local runtime_name
    runtime_name="$(basename "$runtime_archive")"
    cp "$runtime_archive" "$tools_dir/$runtime_name"
    RUNTIME_ARCHIVE_RESOLVED="$tools_dir/$runtime_name"
  fi
}

write_metadata
capture_tool_versions

echo "=== Native ABI evidence packet ==="
echo "out:    $OUT_ABS"
echo "runs:   $RUNS"
echo "python: $PYTHON_BIN"

PERRY_BIN_RESOLVED="$(resolve_perry)"
if [[ -z "$PERRY_ARG" ]]; then
  packet_build=(cargo build -p perry -p perry-runtime)
  case "$PERRY_BIN_RESOLVED" in
    "$ROOT/target/release/"*) packet_build=(cargo build --release -p perry -p perry-runtime) ;;
  esac
  run_logged "packet" "build" "$OUT_ABS/logs/build.log" "${packet_build[@]}"
  PERRY_BIN_RESOLVED="$(resolve_perry)"
elif [[ ! -x "$PERRY_BIN_RESOLVED" ]]; then
  run_logged "packet" "build" "$OUT_ABS/logs/build.log" cargo build -p perry
else
  record_command "packet" "build" "skipped" 0 "" "using explicit Perry binary"
fi

if [[ ! -x "$PERRY_BIN_RESOLVED" ]]; then
  record_command "packet" "resolve_perry" "fail" 1 "" "Perry binary not found at $PERRY_BIN_RESOLVED"
else
  record_command "packet" "resolve_perry" "pass" 0 "" "$PERRY_BIN_RESOLVED"
fi

RUNTIME_ARCHIVE_RESOLVED="$(resolve_runtime_archive "$PERRY_BIN_RESOLVED")"
if [[ -z "$PERRY_ARG" && -f "$RUNTIME_ARCHIVE_RESOLVED" ]]; then
  record_command "packet" "build_runtime_archive" "skipped" 0 "" "built by packet build"
elif [[ ! -f "$RUNTIME_ARCHIVE_RESOLVED" ]]; then
  runtime_build=(cargo build -p perry-runtime)
  case "$PERRY_BIN_RESOLVED" in
    "$ROOT/target/release/"*) runtime_build=(cargo build --release -p perry-runtime) ;;
  esac
  run_logged "packet" "build_runtime_archive" "$OUT_ABS/logs/build-runtime-archive.log" \
    "${runtime_build[@]}"
  RUNTIME_ARCHIVE_RESOLVED="$(resolve_runtime_archive "$PERRY_BIN_RESOLVED")"
else
  record_command "packet" "build_runtime_archive" "skipped" 0 "" "using existing runtime archive"
fi

snapshot_tool_artifacts "$PERRY_BIN_RESOLVED" "$RUNTIME_ARCHIVE_RESOLVED"

"$PYTHON_BIN" - "$METADATA" "$PERRY_BIN_RESOLVED" "$RUNTIME_ARCHIVE_RESOLVED" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
repo = Path.cwd()
data = json.loads(path.read_text(encoding="utf-8"))
data["perry"] = sys.argv[2]
data["runtime_archive"] = sys.argv[3]
archive = Path(sys.argv[3])
if archive.exists():
    data["runtime_archive_sha256"] = hashlib.sha256(archive.read_bytes()).hexdigest()

runtime_inputs = []
for base in (repo / "crates" / "perry-runtime" / "src",):
    runtime_inputs.extend(sorted(p for p in base.rglob("*") if p.is_file()))
for extra in (
    repo / "crates" / "perry-runtime" / "Cargo.toml",
    repo / "crates" / "perry-runtime" / "build.rs",
    repo / "scripts" / "check_runtime_symbols.sh",
):
    if extra.exists():
        runtime_inputs.append(extra)
digest = hashlib.sha256()
for source in sorted(set(runtime_inputs)):
    rel = source.relative_to(repo).as_posix()
    digest.update(rel.encode("utf-8"))
    digest.update(b"\0")
    digest.update(hashlib.sha256(source.read_bytes()).digest())
    digest.update(b"\0")
data["runtime_source_digest"] = digest.hexdigest()
data["runtime_source_digest_inputs"] = len(set(runtime_inputs))
path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

run_logged "release" "runtime_symbols" "$OUT_ABS/logs/runtime-symbols.log" \
  bash scripts/check_runtime_symbols.sh "$RUNTIME_ARCHIVE_RESOLVED"

run_logged "correctness" "native_abi_contract" "$OUT_ABS/correctness/native-abi-contract/command.log" \
  env "PERRY=$PERRY_BIN_RESOLVED" "PERRY_NATIVE_ABI_EVIDENCE_DIR=$OUT_ABS/correctness/native-abi-contract" \
  bash tests/test_native_abi_contract.sh

run_logged "correctness" "c_layout_pod_records" "$OUT_ABS/correctness/c-layout-pod-records/command.log" \
  env "PERRY=$PERRY_BIN_RESOLVED" "PERRY_NATIVE_ABI_EVIDENCE_DIR=$OUT_ABS/correctness/c-layout-pod-records" \
  bash tests/test_c_layout_pod_records.sh

if "$PYTHON_BIN" - <<'PY'
import sys
raise SystemExit(0 if sys.version_info >= (3, 11) else 1)
PY
then
  run_logged "packet" "compiler_output" "$OUT_ABS/logs/compiler-output-native-abi-proof.log" \
    "$PYTHON_BIN" scripts/compiler_output_regression.py suite \
      --suite native-abi-proof \
      --out-dir "$OUT_ABS/compiler-output/native-abi-proof" \
      --perry "$PERRY_BIN_RESOLVED" \
      --runs "$RUNS" \
      --benchmark-mode smoke \
      --gate \
      --perf-counters off \
      --print-summary
else
  record_command "packet" "compiler_output" "fail" 2 "" "Python 3.11+ is required for compiler-output TOML parsing"
fi

run_logged "runtime" "native_async" "$OUT_ABS/logs/native-async-cargo-test.log" \
  cargo test -p perry-runtime native_async -- --nocapture

REPORT_ARGS=(--root "$OUT_ABS" --metadata "$METADATA" --repo-root "$ROOT")
if [[ "$GATE" -eq 1 ]]; then
  REPORT_ARGS+=(--gate)
fi
run_logged "packet" "report" "$OUT_ABS/logs/native-abi-evidence-report.log" \
  "$PYTHON_BIN" scripts/native_abi_evidence_report.py "${REPORT_ARGS[@]}"

STATUS="$("$PYTHON_BIN" - "$OUT_ABS/native-abi-evidence.json" <<'PY'
import json
import sys
from pathlib import Path
path = Path(sys.argv[1])
if not path.exists():
    print("fail")
else:
    print(json.loads(path.read_text(encoding="utf-8")).get("status", "fail"))
PY
)"

echo "json: $OUT_ABS/native-abi-evidence.json"
echo "md:   $OUT_ABS/native-abi-evidence.md"
echo "status: $STATUS"

if [[ "$GATE" -eq 1 && "$STATUS" != "pass" ]]; then
  exit 1
fi
exit 0
