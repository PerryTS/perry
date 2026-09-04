#!/usr/bin/env python3
"""Keep compiler-output subjects observable after a sibling step fails.

GitHub Actions implicitly applies ``success()`` to a step without a status
check.  In ``compiler-output-regression`` that let a failing native-region
proof hide the native-ABI proof (and every later subject) for three weeks
(#8855).  The job intentionally shares one runner, so each independent subject
uses ``!cancelled()`` to bypass that implicit condition while retaining normal
failing-step and failing-job semantics.

This checker runs in required ``lint``.  It owns the ordered subject inventory,
the build prerequisite, and the exact guards, so a future subject cannot be
added below the build with the old fail-fast default.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "test.yml"
JOB_ID = "compiler-output-regression"
BUILD_STEP = "Build compiler"
BUILD_STEP_ID = "compiler_output_build"
UPLOAD_STEP = "Upload compiler-output artifacts"
UNIT_TEST_GUARD = "${{ !cancelled() }}"
COMPILER_SUBJECT_GUARD = (
    "${{ !cancelled() && steps.compiler_output_build.outcome == 'success' }}"
)
UNIT_TEST_SUBJECTS = (
    "Run harness unit tests",
    "Run native ABI evidence report unit tests",
)
COMPILER_SUBJECTS = (
    "Gate native-region proof compiler output",
    "Gate native-ABI proof compiler output",
    "Gate typed feedback runtime evidence",
    "Gate positive vectorization compiler output",
    "Gate HIR fact rewrite compiler output",
    "Gate FP contraction compiler output",
    "Gate fast-math no-contraction compiler output",
)
SUBJECTS = UNIT_TEST_SUBJECTS + COMPILER_SUBJECTS


@dataclass(frozen=True)
class Step:
    name: str
    step_id: str
    condition: str
    continue_on_error: str


def _job_body(text: str, job_id: str) -> str:
    match = re.search(rf"^  {re.escape(job_id)}:\s*$", text, re.MULTILINE)
    if match is None:
        return ""
    body = text[match.end() :]
    next_job = re.search(r"^  [A-Za-z0-9_-]+:\s*$", body, re.MULTILINE)
    return body[: next_job.start()] if next_job is not None else body


def _field(chunk: str, key: str) -> str:
    normalized = "        " + chunk
    match = re.search(
        rf"^        {re.escape(key)}\s*:\s*(.*?)\s*$",
        normalized,
        re.MULTILINE,
    )
    return match.group(1) if match is not None else ""


def _steps(job_body: str) -> list[Step]:
    parsed = []
    for chunk in re.split(r"^      - ", job_body, flags=re.MULTILINE)[1:]:
        parsed.append(
            Step(
                name=_field(chunk, "name"),
                step_id=_field(chunk, "id"),
                condition=_field(chunk, "if"),
                continue_on_error=_field(chunk, "continue-on-error"),
            )
        )
    return parsed


def check_workflow(text: str) -> list[str]:
    problems: list[str] = []
    body = _job_body(text, JOB_ID)
    if not body:
        return [f"job `{JOB_ID}` is missing"]

    job_continue_on_error = re.search(
        r"^    continue-on-error\s*:\s*([^\n]+?)\s*$", body, re.MULTILINE
    )
    if job_continue_on_error is not None:
        problems.append(
            f"job `{JOB_ID}` must not set `continue-on-error` "
            f"(`{job_continue_on_error.group(1)}`); subject failures must still "
            "fail the job"
        )

    steps = _steps(body)
    by_name: dict[str, list[tuple[int, Step]]] = {}
    for index, step in enumerate(steps):
        if step.name:
            by_name.setdefault(step.name, []).append((index, step))

    def unique_step(name: str) -> tuple[int, Step] | None:
        matches = by_name.get(name, [])
        if len(matches) != 1:
            problems.append(f"expected exactly one `{name}` step, found {len(matches)}")
            return None
        return matches[0]

    build = unique_step(BUILD_STEP)
    upload = unique_step(UPLOAD_STEP)
    if build is None or upload is None:
        return problems

    build_index, build_step = build
    upload_index, _ = upload
    if build_step.step_id != BUILD_STEP_ID:
        problems.append(
            f"`{BUILD_STEP}` must have id `{BUILD_STEP_ID}`, found "
            f"`{build_step.step_id or '(none)'}`"
        )
    if build_index >= upload_index:
        problems.append(f"`{BUILD_STEP}` must precede `{UPLOAD_STEP}`")
        return problems

    observed = tuple(
        step.name or "<unnamed step>"
        for step in steps[build_index + 1 : upload_index]
    )
    if observed != SUBJECTS:
        problems.append(
            "post-build subject inventory drifted; expected "
            f"{list(SUBJECTS)!r}, found {list(observed)!r}"
        )

    expected_guards = {
        **{name: UNIT_TEST_GUARD for name in UNIT_TEST_SUBJECTS},
        **{name: COMPILER_SUBJECT_GUARD for name in COMPILER_SUBJECTS},
    }
    for name, expected_guard in expected_guards.items():
        match = unique_step(name)
        if match is None:
            continue
        _index, step = match
        if step.condition != expected_guard:
            problems.append(
                f"`{name}` must use `if: {expected_guard}` so a sibling failure "
                f"cannot hide it; found `{step.condition or '(default success())'}`"
            )
        if step.continue_on_error:
            problems.append(
                f"`{name}` must not set `continue-on-error` "
                f"(`{step.continue_on_error}`); its failure must remain visible "
                "in the job result"
            )

    return problems


def _fixture() -> str:
    lines = [
        "name: fixture",
        "jobs:",
        f"  {JOB_ID}:",
        "    runs-on: ubuntu-latest",
        "    steps:",
        f"      - name: {BUILD_STEP}",
        f"        id: {BUILD_STEP_ID}",
        "        run: cargo build -p perry",
    ]
    for name in SUBJECTS:
        guard = UNIT_TEST_GUARD if name in UNIT_TEST_SUBJECTS else COMPILER_SUBJECT_GUARD
        lines.extend((f"      - name: {name}", f"        if: {guard}", "        run: true"))
    lines.extend((f"      - name: {UPLOAD_STEP}", "        if: always()", "        run: true"))
    return "\n".join(lines) + "\n"


def _self_test() -> int:
    clean = _fixture()
    failures: list[str] = []
    cases = 0

    def expect(name: str, text: str, wanted: str | None) -> None:
        nonlocal cases
        cases += 1
        got = check_workflow(text)
        if wanted is None and got:
            failures.append(f"{name}: expected clean, got {got}")
        elif wanted is not None and not any(wanted in problem for problem in got):
            failures.append(f"{name}: expected {wanted!r}, got {got}")

    expect("clean", clean, None)
    expect(
        "default success condition",
        clean.replace(
            f"      - name: {COMPILER_SUBJECTS[1]}\n        if: {COMPILER_SUBJECT_GUARD}\n",
            f"      - name: {COMPILER_SUBJECTS[1]}\n",
        ),
        "default success()",
    )
    expect(
        "continue-on-error",
        clean.replace(
            f"      - name: {COMPILER_SUBJECTS[0]}\n",
            f"      - name: {COMPILER_SUBJECTS[0]}\n        continue-on-error: true\n",
        ),
        "must not set `continue-on-error`",
    )
    expect(
        "job continue-on-error",
        clean.replace(
            "    runs-on: ubuntu-latest",
            "    continue-on-error: ${{ true }}\n    runs-on: ubuntu-latest",
        ),
        "subject failures must still fail the job",
    )
    expect(
        "wrong build id",
        clean.replace(f"        id: {BUILD_STEP_ID}", "        id: build"),
        f"must have id `{BUILD_STEP_ID}`",
    )
    expect(
        "combined FP subjects",
        clean.replace(COMPILER_SUBJECTS[-2], "Gate FP contraction modes", 1),
        "subject inventory drifted",
    )
    expect(
        "new unclassified subject",
        clean.replace(
            f"      - name: {UPLOAD_STEP}",
            "      - name: Gate a new compiler-output subject\n"
            f"        if: {COMPILER_SUBJECT_GUARD}\n"
            "        run: true\n"
            f"      - name: {UPLOAD_STEP}",
        ),
        "subject inventory drifted",
    )
    expect("missing job", clean.replace(f"  {JOB_ID}:", "  renamed:"), "is missing")

    if failures:
        print("compiler-output step liveness self-test FAILED:", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        return 1
    print(f"compiler-output step liveness self-test passed ({cases} cases)")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return _self_test()

    problems = check_workflow(WORKFLOW.read_text(encoding="utf-8"))
    if problems:
        print("compiler-output step liveness check FAILED:", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        return 1
    print(f"compiler-output step liveness check passed ({len(SUBJECTS)} subjects)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
