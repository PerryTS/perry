from __future__ import annotations

import argparse
import copy
import json
import os
import re
import shutil
import subprocess
from pathlib import Path
from typing import Any

from .analyzers import (
    disassemble_object,
    parse_kept_paths,
    parse_target_triple,
    parse_vectorization_remarks,
    region_counters,
    run_benchmark,
    run_perf_stat,
    runtime_counter_summary,
    structural_counters,
)
from .common import (
    DEFAULT_BENCHMARK_RUNS,
    REPO_ROOT,
    SCHEMA_VERSION,
    HarnessError,
    read_json,
    relpath,
    run_command,
    utc_now,
    write_text,
)
from .spec import WORKLOADS
from .verification import TRACE_RUNTIME_BUDGET_FIELDS, verify_artifacts


SUITES: dict[str, list[str]] = {
    "native-region-proof": [
        "h1_native_rep_equivalence",
        "h1_buffer_alias_negative",
        "image_convolution",
        "loop_data_dependent",
        "numeric_arrays",
        "packed_f64_loop_versioning",
        "packed_f64_loop_versioning_negative",
        "dynamic_fractional_array_index",
        "loop_bound_semantics",
        "raw_numeric_object_fields",
        "scalar_replacement_literals",
    ],
    "native-abi-proof": [
        "h1_native_rep_equivalence",
        "h1_buffer_alias_negative",
        "numeric_arrays",
        "raw_numeric_object_fields",
        "scalar_replacement_literals",
        "width_aware_buffer_kernels",
        "native_owned_typed_views",
        "native_pod_layout_constants",
        "native_memory_bulk_fill",
        "native_memory_fixture",
        "native_abi_packet_typed",
        "native_abi_packet_control",
    ],
}


def resolve_perry(arg: str | None) -> list[str]:
    candidate = arg or os.environ.get("PERRY_BIN")
    if candidate:
        path = Path(candidate)
        if path.is_absolute():
            return [str(path)]
        if path.exists() or os.sep in candidate:
            return [str((REPO_ROOT / path).resolve())]
        return [candidate]
    for path in (REPO_ROOT / "target/release/perry", REPO_ROOT / "target/debug/perry"):
        if path.exists():
            return [str(path)]
    return ["cargo", "run", "--quiet", "-p", "perry", "--"]


def resolve_clang(arg: str | None) -> str:
    clang = arg or os.environ.get("PERRY_LLVM_CLANG") or os.environ.get("CLANG") or shutil.which("clang")
    if not clang:
        raise HarnessError("clang is required to emit optimized IR analysis")
    return clang


def compiler_version(argv: list[str]) -> str:
    try:
        proc = subprocess.run(
            argv + ["--version"],
            cwd=str(REPO_ROOT),
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=20,
        )
        return proc.stdout.strip().splitlines()[0] if proc.stdout.strip() else "unknown"
    except Exception as exc:  # pragma: no cover - defensive metadata only.
        return f"unavailable: {type(exc).__name__}: {exc}"


def resolve_benchmark_runs(args: argparse.Namespace) -> int:
    if args.runs is not None:
        runs = int(args.runs)
    else:
        runs = DEFAULT_BENCHMARK_RUNS[args.benchmark_mode]
    if runs < 1:
        raise HarnessError("--runs must be at least 1")
    return runs


def _compile_env(clang: str, *, enable_gc_trace: bool = False) -> dict[str, str]:
    env = {**os.environ, "PERRY_LLVM_KEEP_IR": "1", "PERRY_NO_CACHE": "1"}
    env["PERRY_LLVM_CLANG"] = clang
    if enable_gc_trace:
        env["PERRY_GC_TRACE"] = "1"
    return env


def _append_common_perry_flags(cmd: list[str], args: argparse.Namespace) -> None:
    if args.target:
        cmd.extend(["--target", args.target])
    if args.fast_math:
        cmd.append("--fast-math")
    if args.fp_contract:
        cmd.append(f"--fp-contract={args.fp_contract}")
    if getattr(args, "verify_native_regions", False):
        cmd.append("--verify-native-regions")


def _metadata_candidates(object_path: Path, explicit: Path | None) -> list[Path]:
    candidates: list[Path] = []
    if explicit is not None:
        candidates.append(explicit)
    candidates.append(Path(str(object_path) + ".compile-plan.json"))
    candidates.append(object_path.with_suffix(".compile-plan.json"))
    return candidates


def _load_compile_metadata(object_path: Path, explicit: Path | None) -> dict[str, Any]:
    for candidate in _metadata_candidates(object_path, explicit):
        if candidate.exists():
            data = read_json(candidate)
            data["metadata_path"] = str(candidate)
            return data
    return {
        "schema_version": SCHEMA_VERSION,
        "metadata_path": None,
        "object_path": str(object_path),
        "effective_target": "",
        "clang_path": "",
        "clang_args": [],
        "native_tuning_arg": None,
        "stderr_remarks_path": None,
        "analysis_clang_args": [],
    }


def _analysis_args_from_metadata(
    metadata: dict[str, Any], extra_args: list[str]
) -> list[str]:
    args = list(metadata.get("analysis_clang_args") or [])
    if not args:
        args = ["-O3", "-fno-math-errno"]
        native = metadata.get("native_tuning_arg")
        if native:
            args.append(str(native))
        target = metadata.get("effective_target")
        if target:
            args.extend(["-target", str(target)])
    args.extend(extra_args)
    return args


def capture(args: argparse.Namespace) -> int:
    if args.workload not in WORKLOADS:
        raise HarnessError(f"unknown workload {args.workload!r}")

    workload_info = WORKLOADS[args.workload]
    source = (REPO_ROOT / workload_info["source"]).resolve()
    if not source.exists():
        raise HarnessError(f"source not found: {source}")

    out_dir = (
        Path(args.out_dir)
        if args.out_dir
        else REPO_ROOT / "target/compiler-output-regression" / args.workload
    )
    if not out_dir.is_absolute():
        out_dir = REPO_ROOT / out_dir
    out_dir = out_dir.resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    perry = resolve_perry(args.perry)
    clang = resolve_clang(args.clang)
    analysis_extra_clang_args = list(args.clang_arg or [])
    runs = resolve_benchmark_runs(args)
    trace_budget_fields = set(
        workload_info.get("runtime_budgets", {})
    ).intersection(TRACE_RUNTIME_BUDGET_FIELDS)
    compile_gc_trace = bool(trace_budget_fields and not args.no_gc_trace)
    binary = (out_dir / args.workload).resolve()

    commands: dict[str, Any] = {}

    hir_stdout = out_dir / "hir.txt"
    hir_stderr = out_dir / "hir.stderr"
    hir_cmd = perry + [
        "compile",
        str(source),
        "-o",
        str(out_dir / "hir-probe.o"),
        "--print-hir",
        "--no-link",
        "--no-cache",
    ]
    _append_common_perry_flags(hir_cmd, args)
    commands["hir"] = run_command(
        hir_cmd,
        cwd=out_dir,
        env=_compile_env(clang, enable_gc_trace=compile_gc_trace),
        timeout=args.compile_timeout,
        stdout_path=hir_stdout,
        stderr_path=hir_stderr,
    ).to_json()

    compile_stdout = out_dir / "compile.stdout"
    compile_stderr = out_dir / "compile.stderr"
    compile_cmd = perry + [
        "compile",
        str(source),
        "-o",
        str(binary),
        "--no-cache",
    ]
    _append_common_perry_flags(compile_cmd, args)
    commands["compile"] = run_command(
        compile_cmd,
        cwd=out_dir,
        env=_compile_env(clang, enable_gc_trace=compile_gc_trace),
        timeout=args.compile_timeout,
        stdout_path=compile_stdout,
        stderr_path=compile_stderr,
    ).to_json()

    kept_irs, kept_objects, kept_metadata, kept_native_reps = parse_kept_paths(
        compile_stdout.read_text(encoding="utf-8")
        + "\n"
        + compile_stderr.read_text(encoding="utf-8")
    )
    if not kept_irs:
        raise HarnessError("PERRY_LLVM_KEEP_IR did not report a retained LLVM IR path")
    if not kept_objects:
        raise HarnessError("PERRY_LLVM_KEEP_IR did not report a retained object path")

    primary_ir = kept_irs[0]

    ir_before_path = out_dir / "llvm-before-opt.ll"
    shutil.copyfile(primary_ir, ir_before_path)
    for index, path in enumerate(kept_irs[1:], start=1):
        shutil.copyfile(path, out_dir / f"llvm-before-opt-{index}.ll")

    copied_objects: list[dict[str, Any]] = []
    for index, path in enumerate(kept_objects):
        if not path.exists():
            continue
        object_artifact = out_dir / f"object-{index}.o"
        shutil.copyfile(path, object_artifact)
        explicit_meta = kept_metadata[index] if index < len(kept_metadata) else None
        metadata = _load_compile_metadata(path, explicit_meta)
        metadata_artifact = out_dir / f"object-{index}.compile-plan.json"
        write_text(metadata_artifact, json.dumps(metadata, indent=2, sort_keys=True) + "\n")
        copied_objects.append(
            {
                "index": index,
                "retained_object_path": str(path),
                "object_artifact": str(object_artifact),
                "compile_plan_metadata": metadata,
                "compile_plan_artifact": str(metadata_artifact),
            }
        )
    if not copied_objects:
        raise HarnessError("no retained objects existed on disk")

    primary_object_artifact = Path(copied_objects[0]["object_artifact"])
    compile_metadata = copied_objects[0]["compile_plan_metadata"]

    copied_native_reps: list[dict[str, Any]] = []
    for index, path in enumerate(kept_native_reps):
        if not path.exists():
            continue
        artifact = out_dir / f"native-reps-{index}.json"
        shutil.copyfile(path, artifact)
        if index == 0:
            shutil.copyfile(path, out_dir / "native-reps.json")
        copied_native_reps.append(
            {
                "index": index,
                "retained_native_reps_path": str(path),
                "native_reps_artifact": str(artifact),
            }
        )
    native_reps = [
        read_json(Path(row["native_reps_artifact"])) for row in copied_native_reps
    ]

    ir_before = ir_before_path.read_text(encoding="utf-8")
    target = (
        compile_metadata.get("effective_target")
        or args.target
        or parse_target_triple(ir_before)
        or "x86_64-unknown-linux-gnu"
    )
    compile_clang = compile_metadata.get("clang_path") or clang
    compile_clang_args = list(compile_metadata.get("clang_args") or [])
    analysis_args = _analysis_args_from_metadata(compile_metadata, analysis_extra_clang_args)

    ir_after_path = out_dir / "llvm-after-opt.analysis.ll"
    opt_remarks_path = out_dir / "llvm-vectorization-remarks.stderr"
    opt_cmd = [
        str(compile_clang),
        "-S",
        "-emit-llvm",
        *analysis_args,
        "-Rpass=loop-vectorize",
        "-Rpass-missed=loop-vectorize",
        "-Rpass-analysis=loop-vectorize",
        str(ir_before_path),
        "-o",
        str(ir_after_path),
    ]
    commands["llvm_after_opt_analysis"] = run_command(
        opt_cmd,
        cwd=out_dir,
        timeout=args.compile_timeout,
        stderr_path=opt_remarks_path,
    ).to_json()

    disassembly_path = out_dir / "object-disassembly.s"
    commands["object_disassembly"] = disassemble_object(
        primary_object_artifact,
        output_path=disassembly_path,
        cwd=out_dir,
        timeout=args.compile_timeout,
    )
    # Compatibility alias for older artifact readers. The manifest marks the
    # real source as object_disassembly.
    shutil.copyfile(disassembly_path, out_dir / "assembly.s")

    benchmark = None
    if not args.skip_run:
        benchmark = run_benchmark(
            binary,
            out_dir=out_dir,
            runs=runs,
            timeout=args.run_timeout,
            enable_gc_trace=not args.no_gc_trace,
            benchmark_mode=args.benchmark_mode,
        )

    perf_stat = None
    if not args.skip_run and args.perf_counters != "off":
        perf_stat = run_perf_stat(binary, out_dir=out_dir, timeout=args.run_timeout)
        if args.perf_counters == "on" and not perf_stat.get("available"):
            raise HarnessError(f"perf stat unavailable: {perf_stat.get('reason')}")

    ir_after = ir_after_path.read_text(encoding="utf-8")
    assembly = disassembly_path.read_text(encoding="utf-8")
    vectorization = parse_vectorization_remarks(
        opt_remarks_path.read_text(encoding="utf-8")
        if opt_remarks_path.exists()
        else ""
    )
    counters = structural_counters(ir_before, ir_after, assembly)
    regions = region_counters(args.workload, ir_after, WORKLOADS)
    runtime_summary = runtime_counter_summary(benchmark, counters)
    fp_contract_mode = (
        args.fp_contract if args.fp_contract else ("fast" if args.fast_math else "off")
    )
    verification = verify_artifacts(
        workload=args.workload,
        ir_before=ir_before,
        ir_after=ir_after,
        assembly=assembly,
        benchmark=benchmark,
        vectorization=vectorization,
        counters=counters,
        runtime_summary=runtime_summary,
        fp_contract_mode=fp_contract_mode,
        target=str(target),
        clang_args=compile_clang_args,
        expect_fma=args.expect_fma,
        native_reps=native_reps,
    )

    manifest = {
        "schema_version": SCHEMA_VERSION,
        "generated_at": utc_now(),
        "workload": args.workload,
        "workload_kind": workload_info["kind"],
        "source": relpath(source),
        "target": target,
        "fp_modes": {
            "fast_math": bool(args.fast_math),
            "fp_contract": fp_contract_mode,
        },
        "compile_plan": compile_metadata,
        "analysis_extra_clang_args": analysis_extra_clang_args,
        "benchmark_settings": {
            "benchmark_mode": args.benchmark_mode,
            "runs": runs,
            "user_supplied_runs": args.runs is not None,
        },
        "tool_versions": {
            "perry": compiler_version(perry),
            "clang": compiler_version([str(compile_clang)]),
        },
        "commands": commands,
        "artifacts": {
            "hir": str(hir_stdout),
            "llvm_before_opt": str(ir_before_path),
            "llvm_after_opt_analysis": {
                "path": str(ir_after_path),
                "role": "analysis_only",
                "source_ir": str(ir_before_path),
            },
            "object_disassembly": {
                "path": str(disassembly_path),
                "role": "executed_object_disassembly",
                "source_object": str(primary_object_artifact),
            },
            "vectorization_remarks": str(opt_remarks_path),
            "binary": str(binary),
            "retained_objects": copied_objects,
            "native_reps": copied_native_reps,
        },
        "benchmark": benchmark,
        "perf_stat": perf_stat,
        "vectorization_remarks": vectorization,
        "counters": counters,
        "regions": regions,
        "runtime_counter_summary": runtime_summary,
        "verification": verification,
    }

    manifest_path = out_dir / "manifest.json"
    verification_path = out_dir / "structural-report.json"
    write_text(manifest_path, json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    write_text(
        verification_path,
        json.dumps(verification, indent=2, sort_keys=True) + "\n",
    )

    if args.print_summary:
        print(json.dumps({"manifest": str(manifest_path), **verification}, indent=2))

    if args.gate and verification["status"] != "pass":
        return 1
    return 0


def capture_suite(args: argparse.Namespace) -> int:
    workloads = SUITES.get(args.suite)
    if workloads is None:
        raise HarnessError(f"unknown suite {args.suite!r}")

    suite_root = (
        Path(args.out_dir)
        if args.out_dir
        else REPO_ROOT / "target/compiler-output-regression" / args.suite
    )
    if not suite_root.is_absolute():
        suite_root = REPO_ROOT / suite_root
    suite_root = suite_root.resolve()
    suite_root.mkdir(parents=True, exist_ok=True)

    rows: list[dict[str, Any]] = []
    for workload in workloads:
        workload_out = suite_root / workload
        per_workload = copy.copy(args)
        per_workload.workload = workload
        per_workload.out_dir = str(workload_out)
        per_workload.gate = bool(args.gate)
        per_workload.print_summary = False
        per_workload.verify_native_regions = True
        try:
            exit_code = capture(per_workload)
            report_path = workload_out / "structural-report.json"
            report = read_json(report_path) if report_path.exists() else {}
            status = str(report.get("status") or ("pass" if exit_code == 0 else "fail"))
            errors = list(report.get("errors") or [])
        except HarnessError as exc:
            exit_code = 2
            report_path = workload_out / "structural-report.json"
            status = "fail"
            errors = [str(exc)]
        rows.append(
            {
                "workload": workload,
                "status": status,
                "exit_code": exit_code,
                "artifact_dir": str(workload_out),
                "structural_report": str(report_path),
                "errors": errors,
            }
        )

    failed = [row for row in rows if row["status"] != "pass" or row["exit_code"] != 0]
    suite_report = {
        "schema_version": SCHEMA_VERSION,
        "generated_at": utc_now(),
        "suite": args.suite,
        "status": "pass" if not failed else "fail",
        "workloads": rows,
        "failed_workloads": [row["workload"] for row in failed],
    }
    report_path = suite_root / "suite-report.json"
    write_text(report_path, json.dumps(suite_report, indent=2, sort_keys=True) + "\n")

    if args.print_summary:
        print(json.dumps({"suite_report": str(report_path), **suite_report}, indent=2))

    return 1 if failed else 0


def _resolve_artifact_path(root: Path, value: Any) -> Path | None:
    if not isinstance(value, str) or not value:
        return None
    path = Path(value)
    return path if path.is_absolute() else root / path


def _native_rep_sort_key(path: Path) -> tuple[int, int | str]:
    match = re.fullmatch(r"native-reps-(\d+)\.json", path.name)
    if match:
        return (0, int(match.group(1)))
    return (1, path.name)


def _native_rep_artifact_paths(root: Path, manifest: dict[str, Any]) -> list[Path]:
    paths: list[Path] = []
    artifacts = manifest.get("artifacts") if isinstance(manifest, dict) else {}
    retained = artifacts.get("native_reps", []) if isinstance(artifacts, dict) else []
    if isinstance(retained, list):
        missing: list[str] = []
        for row in retained:
            if not isinstance(row, dict):
                continue
            path = _resolve_artifact_path(root, row.get("native_reps_artifact"))
            if not path:
                continue
            if path.exists():
                paths.append(path)
            else:
                missing.append(str(path))
        if missing:
            raise HarnessError(
                "missing native reps artifacts listed in manifest: "
                + ", ".join(missing)
            )
    if not paths:
        paths.extend(sorted(root.glob("native-reps-*.json"), key=_native_rep_sort_key))
    alias = root / "native-reps.json"
    if alias.exists() and not paths:
        paths.append(alias)

    deduped: list[Path] = []
    seen: set[Path] = set()
    for path in paths:
        resolved = path.resolve()
        if resolved in seen:
            continue
        seen.add(resolved)
        deduped.append(path)
    return deduped


def _load_native_rep_artifacts(root: Path, manifest: dict[str, Any]) -> list[dict[str, Any]]:
    return [read_json(path) for path in _native_rep_artifact_paths(root, manifest)]


def verify_existing(args: argparse.Namespace) -> int:
    root = Path(args.artifact_dir)
    before = root / "llvm-before-opt.ll"
    after = root / "llvm-after-opt.analysis.ll"
    if not after.exists():
        after = root / "llvm-after-opt.ll"
    asm = root / "object-disassembly.s"
    if not asm.exists():
        asm = root / "assembly.s"
    missing = [str(path) for path in (before, after, asm) if not path.exists()]
    if missing:
        raise HarnessError(f"missing artifacts: {', '.join(missing)}")

    manifest = read_json(root / "manifest.json") if (root / "manifest.json").exists() else {}
    compile_plan = manifest.get("compile_plan") or {}
    remarks = root / "llvm-vectorization-remarks.stderr"
    vectorization = parse_vectorization_remarks(
        remarks.read_text(encoding="utf-8") if remarks.exists() else ""
    )
    ir_before = before.read_text(encoding="utf-8")
    ir_after = after.read_text(encoding="utf-8")
    assembly = asm.read_text(encoding="utf-8")
    counters = structural_counters(ir_before, ir_after, assembly)
    benchmark = (
        manifest.get("benchmark") if isinstance(manifest.get("benchmark"), dict) else None
    )
    runtime_summary = runtime_counter_summary(benchmark, counters)
    target = (
        args.target
        or compile_plan.get("effective_target")
        or parse_target_triple(ir_before)
        or ""
    )
    clang_args = list(compile_plan.get("clang_args") or args.clang_arg or [])
    report = verify_artifacts(
        workload=args.workload,
        ir_before=ir_before,
        ir_after=ir_after,
        assembly=assembly,
        benchmark=benchmark,
        vectorization=vectorization,
        counters=counters,
        runtime_summary=runtime_summary,
        fp_contract_mode=args.fp_contract or "off",
        target=str(target),
        clang_args=clang_args,
        expect_fma=args.expect_fma,
        native_reps=_load_native_rep_artifacts(root, manifest),
    )
    output = root / "structural-report.json"
    write_text(output, json.dumps(report, indent=2, sort_keys=True) + "\n")
    if args.print_summary:
        print(json.dumps(report, indent=2))
    return 1 if args.gate and report["status"] != "pass" else 0
