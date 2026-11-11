#!/usr/bin/env python3
"""Runnable controls for runtime claim reports, copies and compile-time coverage."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess

FIELDS = {"marker", "module", "byte_offset", "key", "verdict", "predicate",
          "proven_receivers", "observed_receiver", "observed_member",
          "observed_callee", "copy", "compile_sha"}


def no_duplicates(pairs):
    result = {}
    for key, value in pairs:
        assert key not in result, f"duplicate JSON field: {key}"
        result[key] = value
    return result


def run(args, **kw):
    return subprocess.run(args, timeout=300, capture_output=True, **kw)


def report(observed):
    assert observed.returncode == 86, (observed.returncode, observed.stdout, observed.stderr)
    lines = observed.stderr.splitlines()
    assert len(lines) == 1, lines
    value = json.loads(lines[0], object_pairs_hook=no_duplicates)
    assert FIELDS <= value.keys() and value["marker"] == "PERRY_FALSE_PROOF", value
    assert value["copy"]["lowered"] and re.fullmatch(r"[0-9a-f]{40}", value["compile_sha"]), value
    return value


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--perry", type=Path, required=True)
    parser.add_argument("--node", default="node")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--canonnum-runtime", type=Path,
                        help="target/release of a runtime built with PERRY_FACTS_SABOTAGE=canonnum "
                             "(scripts/p3-canonnum-runtime.sh); without it only the correct arm runs")
    args = parser.parse_args()
    args.perry = args.perry.resolve()
    args.out = args.out.resolve()
    args.out.mkdir(parents=True, exist_ok=True)
    fixtures = Path(__file__).with_name("verify_facts_s0").resolve()
    native = fixtures / "native-escape"
    built = run(["cc", "-shared", "-fPIC", str(native / "invoke.c"), "-o", str(native / "invoke.node")])
    assert built.returncode == 0, built.stderr
    env = dict(os.environ, PERRY_VERIFY_FACTS="1", PERRY_FACTS_UNITS="0", PERRY_NO_CACHE="1",
               PERRY_DISABLE_BUILD_CACHE="1", PERRY_NO_LINK_CACHE="1", PERRY_NO_ARCHIVE_CACHE="1",
               PERRY_ALLOW_PERRY_FEATURES="1", PERRY_CONTRACTS_REPORT="1")
    env.pop("PERRY_FACTS_SABOTAGE", None)
    results = []

    def save(name, **values):
        results.append(dict(name=name, **values))
        (args.out / "results.json").write_text(json.dumps(results, indent=2) + "\n")

    def compile_(source, name, **overrides):
        exe = args.out / name
        e = dict(env, **overrides)
        compiled = run([str(args.perry), "compile", "--cache-dir", str(args.out / "cache"),
                        str(source), "-o", str(exe)], env=e)
        (args.out / (name + ".compile")).write_bytes(compiled.stdout + compiled.stderr)
        assert compiled.returncode == 0, (name, compiled.stderr)
        return exe, e

    def observe(exe, e, *argv):
        value = run([str(exe), *argv], env=e)
        suffix = "-" + "-".join(argv) if argv else ""
        (args.out / (exe.name + suffix + ".stdout")).write_bytes(value.stdout)
        (args.out / (exe.name + suffix + ".stderr")).write_bytes(value.stderr)
        return value

    def oracle(source, *argv):
        value = run([args.node, "--expose-gc", "--experimental-strip-types", str(source), *argv])
        assert value.returncode == 0, value.stderr
        return value.stdout

    for name, wrong_claim in json.loads((fixtures / "cases.json").read_text()).items():
        source = (native if name == "activation-escape" else fixtures) / (name + ".ts")
        expected = oracle(source)
        for claim in ["", wrong_claim]:
            arm = name + ("-false" if claim else "-correct")
            exe, e = compile_(source, arm, PERRY_FACTS_SABOTAGE=claim)
            observed = observe(exe, e)
            if claim:
                value = report(observed)
                assert value["key"] == "<entry>" and value["observed_receiver"] is None, value
                assert b"BODY:" not in observed.stdout, observed.stdout
            else:
                assert observed.returncode == 0 and observed.stdout == expected, observed
            save(arm, status=observed.returncode)

    for source in sorted(fixtures.glob("existing-*.ts")):
        exe, e = compile_(source, source.stem)
        observed = observe(exe, e)
        assert observed.returncode == 0 and observed.stdout == oracle(source), observed
        save(source.stem, status=0, fixed_on_main=True)

    for name, wrong_claim, predicate in [
        ("receiver-negative", "unchecked", "F4 receiver set"),
        ("kind-negative", "kind:Box.v=num", "F2 kind"),
        ("int32-negative", "range", "int32 input"),
        ("int32-compare-negative", "range", "int32 input"),
        ("arrlen-negative", "arrlen", "array constant length"),
        ("element-read-negative", "arrlen", "proven element read"),
        ("element-read-sparse-birth", "densebirth", "proven element read"),
        ("array-floor-negative", "arrfloor", "proven element read"),
        ("array-append-negative", "arrappend", "int32 input"),
        ("array-cap-negative", "arrcap", "proven element read"),
        ("array-fwd-negative", "arrfwd", "proven element read"),
        ("array-dense-negative", "arrdense", "array rep"),
        ("array-elem-negative", "arrelem", "array rep"),
        ("array-lazy-negative", "arrlazy", "array rep"),
        ("array-front-negative", "arrfront", "array rep"),
        ("const-array-negative", "arrconst", "constant array"),
        ("array-field-negative", "arrfield", "array rep"),
        ("array-b1-negative", "arrb1", "array rep"),
        ("range-sum-negative", "exactrule", "array rep"),
        ("leaving-write-negative", "leavingwrite", "array rep"),
        ("array-base-negative", "arrbase", "array rep"),
        ("array-base-admit-negative", "arrbaseadm", "array rep"),
        ("fn-base-admit-negative", "fnbaseadm", "array rep"),
        ("unsigned-bound-negative", "unsignedbound", "unsigned bound"),
        ("number-call-negative", "numbercall", "numeric result"),
        ("field-read-call-negative", "fieldreadcall", "field read call"),
        ("array-undef-negative", "arrundef", "element floor"),
        ("const-number-negative", "constnum", "constant number"),
        ("signature-negative", "signature", "signature"),
        ("signature-escape-negative", "signature-escape", "signature"),
        ("signature-range-negative", "signature-range", "signature"),
        ("rest-field-negative", "restfield", "field rep"),
        ("rest-field-zero-negative", "restfield", "field rep"),
        ("rest-elem-negative", "restelem", "array rep"),
        ("rest-raw-negative", "restraw", "array rep"),
        ("rest-held-negative", "restheld", "field read call"),
        ("rest-rep-negative", "restrep", "array rep"),
        ("rest-cap-negative", "restcap", "array rep"),
        ("zero-sign-negative", "zerosign", "zero sign"),
        ("zero-sign-elem-negative", "zerosign", "zero sign"),
    ]:
        source = fixtures / (name + ".ts")
        # The array arms written before PR-B (DESIGN-p4l1) test the
        # element-read and int32 witnesses. With `Rep::Arr` on, the array
        # rep witness reports the same false length first, so these arms
        # run with it off to reach the witness each was written for.
        older_array_arm = wrong_claim in ("arrlen", "densebirth", "arrfloor", "arrappend", "arrcap", "arrfwd")
        extra = {"PERRY_FACTS_ARRAY_REPS": "0"} if older_array_arm else {}
        # I32 at rest (PR B): a cell array the sabotaged tolerance makes I32 is
        # stored at rest, and the at-rest store witness reports the -0 first
        # ("array rep"); the arm runs with B off to reach the sign witness.
        if name == "zero-sign-elem-negative":
            extra = {"PERRY_REST_ELEMS": "0"}
        for claim in ["", wrong_claim]:
            arm = name + ("-false" if claim else "-correct")
            exe, e = compile_(source, arm, PERRY_FACTS_SABOTAGE=claim, **extra)
            observed = observe(exe, e)
            if claim:
                value = report(observed)
                assert value["predicate"] == predicate, value
            else:
                assert observed.returncode == 0 and observed.stdout == oracle(source), observed
            save(arm, status=observed.returncode)

    # Value records (DESIGN-p4l4 §10.3, VR-2): check mode keeps records on and
    # claims every instance of an admitted template at birth; the facts say
    # its identity is never compared, so a compiled identity compare that
    # sees a claimed operand reports "value record identity".
    for name, wrong_claim in [
        ("vr-ident-negative", "vrident"),
        ("vr-switch-negative", "vrswitch"),
        ("vr-inline-negative", "vrinline"),
    ]:
        source = fixtures / (name + ".ts")
        for claim in ["", wrong_claim]:
            arm = name + ("-false" if claim else "-correct")
            exe, e = compile_(source, arm, PERRY_FACTS_SABOTAGE=claim, PERRY_FACTS_VALUE_RECORDS="1")
            observed = observe(exe, e)
            if claim:
                value = report(observed)
                assert value["predicate"] == "value record identity", value
            else:
                assert observed.returncode == 0 and observed.stdout == oracle(source), observed
            save(arm, status=observed.returncode)

    # P3 B1 (DESIGN-p3 rev 3.1 §5, REVIEW-p3 D4): this arm sabotages the
    # RUNTIME, not the facts. `js_math_round` built with
    # PERRY_FACTS_SABOTAGE=canonnum returns an INT32-boxed word, and the
    # canonical-number witness must report it with rc 86 exactly.
    source = fixtures / "canonical-number-negative.ts"
    exe, e = compile_(source, "canonical-number-negative-correct")
    observed = observe(exe, e)
    assert observed.returncode == 0 and observed.stdout == oracle(source), observed
    save("canonical-number-negative-correct", status=0)
    if args.canonnum_runtime:
        runtime = str(args.canonnum_runtime.resolve())
        exe, e = compile_(source, "canonical-number-negative-false", PERRY_RUNTIME_DIR=runtime)
        value = report(observe(exe, e))
        assert value["predicate"] == "canonical number", value
        save("canonical-number-negative-false", status=86)

    source = fixtures / "parallel-bodies.ts"
    exe, e = compile_(source, "parallel-bodies")
    observed = observe(exe, e)
    assert observed.returncode == 0 and observed.stdout == oracle(source), observed
    inventory = [json.loads(line) for line in Path(str(exe) + ".facts.jsonl").read_text().splitlines() if line]
    assert any(row.get("check") and row.get("emitted_checks") for row in inventory), inventory
    save("parallel-bodies", status=0)

    source = fixtures / "slot-key.ts"
    exe, e = compile_(source, "slot-key-false", PERRY_FACTS_SABOTAGE="fixed_layout:Box")
    value = report(observe(exe, e))
    assert value["predicate"] == "F3 slot", value
    exe, e = compile_(source, "slot-key-correct")
    observed = observe(exe, e)
    assert observed.returncode == 0 and observed.stdout == oracle(source), observed
    save("slot-key", status=86, key=value["key"])

    for name, wrong_claim in [("copy-store", "kind:Box.v=num"), ("copy-body", "activation:call"), ("copy-inline", "activation:this")]:
        source = fixtures / (name + ".ts")
        exe, e = compile_(source, name, PERRY_FACTS_SABOTAGE=wrong_claim)
        values = [report(observe(exe, e, branch)) for branch in ["first", "second"]]
        if name == "copy-inline":
            assert values[0]["copy"] != values[1]["copy"], values
        inventory = [json.loads(line) for line in Path(str(exe) + ".facts.jsonl").read_text().splitlines() if line]
        emitted = [copy for row in inventory for copy in row.get("emitted_checks", [])]
        assert all(value["copy"] in emitted for value in values), (values, emitted)
        assert values[0]["module"] == values[1]["module"] and values[0]["key"] == values[1]["key"], values
        exe, e = compile_(source, name + "-correct")
        for branch in ["first", "second"]:
            observed = observe(exe, e, branch)
            assert observed.returncode == 0 and observed.stdout == oracle(source, branch), observed
        save(name, copies=[value["copy"] for value in values])

    exe, e = compile_(fixtures / "manifest.ts", "manifest")
    inventory = [json.loads(line) for line in Path(str(exe) + ".facts.jsonl").read_text().splitlines() if line]
    assert len({row["identity"] for row in inventory}) == len(inventory), inventory
    assert all(bool(row.get("check")) + bool(row.get("reason")) + bool(row.get("dead")) == 1 for row in inventory), inventory
    assert all(row.get("emitted_checks") for row in inventory if row.get("check")), inventory
    uncovered = sum(bool(row.get("reason")) for row in inventory)
    assert uncovered == 3, (uncovered, inventory)
    # `value.call(undefined)` is a Number call (P2): consumed and checked.
    assert sum(row.get("consumed", False) for row in inventory) == 3, inventory
    assert any(row.get("check") == "numeric result" and row.get("emitted_checks") for row in inventory), inventory
    save("manifest", consumed=len(inventory), uncovered=uncovered)

    samples = []
    for mode in ["0", "1"]:
        exe, e = compile_(fixtures / "passing.ts", "passing-" + mode, PERRY_VERIFY_FACTS=mode, PERRY_GC_LOG="1")
        observed = observe(exe, e)
        assert observed.returncode == 0 and observed.stdout == oracle(fixtures / "passing.ts"), observed
        measured = observe(exe, e, "measure")
        assert measured.returncode == 0, measured
        collections = len(re.findall(rb"^\[gc-log\]", measured.stderr, re.M))
        allocated = int(re.search(rb"^allocated (\d+)$", measured.stdout, re.M).group(1))
        assert collections > 0, measured.stderr
        samples.append(dict(collections=collections, allocated=allocated))
        if mode == "1":
            checks = re.search(rb"(\d+) checks, 0 violations", measured.stderr)
            assert checks and int(checks.group(1)) >= 1000, measured.stderr
    assert samples[0] == samples[1], samples
    save("passing-path", **samples[1])
    print(f"{len(results)} executable controls passed")


if __name__ == "__main__":
    main()
