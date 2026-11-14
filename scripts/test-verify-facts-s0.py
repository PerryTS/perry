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

    # family_reach (check mode's family claim). Its witnesses exist only in
    # the instrumented runtime variant, which PERRY_GC_INSTRUMENTS=1 links.
    runtime_dir = Path(os.environ.get("PERRY_RUNTIME_DIR", str(args.perry.parent)))
    if (runtime_dir / "libperry_runtime_instr.a").exists():
        instr = dict(PERRY_GC_INSTRUMENTS="1")
        # `aborts`: the family's install follows the fact (the byte cells do,
        # `PERRY_FACTS_FAMILY_LINK`), and the program creates a value of it:
        # without the witnesses the sabotaged `never` leaves the family out
        # of the link, and the birth must abort, not answer.
        for name, family, observed_text, aborts in [
            ("family-bytes-negative", "typedarray", "a byte cell is born", True),
            ("family-bytes-arrive-negative", "typedarray", "a byte cell is born", True),
            ("family-bytes-observe-negative", "typedarray", "is observed", False),
            ("family-bytes-poolsize-negative", "typedarray", "is observed", False),
            ("family-regex-negative", "regex_engine", "a RegExp is born", False),
        ]:
            source = fixtures / (name + ".ts")
            expected = oracle(source)
            exe, e = compile_(source, name + "-correct", **instr)
            observed = observe(exe, e)
            assert observed.returncode == 0 and observed.stdout == expected, observed
            dump = args.out / (name + ".facts")
            exe, e = compile_(source, name + "-false", PERRY_FACTS_SABOTAGE="famreach:" + family,
                              PERRY_FACTS_DUMP=str(dump), **instr)
            assert f"# family_reach {family} never reason=none" in dump.read_text(), name
            value = report(observe(exe, e))
            assert value["verdict"] == "family" and value["predicate"] == "family", value
            member = value["observed_member"] or ""
            assert observed_text in member, value
            assert member.startswith(family + ": ") or value["expectation"] == family, value
            # Without the instrumented variant check mode makes no claim.
            # The link decision still reads the fact: a family the detector
            # selected (these programs name it) that the fact says is never
            # used is left out, and a value of it that is created anyway
            # aborts at the birth (no answer, no rc 0); one that is only
            # observed, or whose install the detector still decides, runs as
            # before.
            exe, e = compile_(source, name + "-default", PERRY_FACTS_SABOTAGE="famreach:" + family)
            observed = observe(exe, e)
            if aborts:
                assert observed.returncode not in (0, 86), observed
                assert observed.stdout == b"", observed
                assert b"family_reach" in observed.stderr and family.encode() in observed.stderr, observed
            else:
                assert observed.returncode == 0 and observed.stdout == expected, observed
            # With the link switch off the sabotaged fact has no consumer.
            exe, e = compile_(source, name + "-nolink", PERRY_FACTS_SABOTAGE="famreach:" + family,
                              PERRY_FACTS_FAMILY_LINK="0")
            observed = observe(exe, e)
            assert observed.returncode == 0 and observed.stdout == expected, observed
            save(name, status=86, default="aborts" if aborts else "runs")
        for name, verdict in [("family-bytes-scratch", "never"), ("family-bytes-unwalked", "live"),
                              ("family-bytes-write-encoding", "never")]:
            source = fixtures / (name + ".ts")
            dump = args.out / (name + ".facts")
            exe, e = compile_(source, name, PERRY_FACTS_DUMP=str(dump), **instr)
            assert f"# family_reach typedarray {verdict} " in dump.read_text(), name
            observed = observe(exe, e)
            if name == "family-bytes-unwalked" and observed.returncode == 86:
                # The spread callback is a call edge the activation reach
                # does not see yet: its own claim ("inactive body") reports.
                # The family claim does not: the text made the family live.
                assert report(observed)["verdict"] == "Activation", observed
            else:
                assert observed.returncode == 0 and observed.stdout == oracle(source), observed
            save(name, status=observed.returncode)
        # The link decision only removes. A program whose family is never
        # and whose text the detector does not match is the detector's
        # binary: no install, no claim. A comparator sort's two scratch
        # cells are no values and run there.
        source = fixtures / "family-bytes-scratch.ts"
        exe, e = compile_(source, "family-bytes-scratch-default", PERRY_KEEP_SYMBOLS="1")
        observed = observe(exe, e)
        assert observed.returncode == 0 and observed.stdout == oracle(source), observed
        symbols = run(["nm", str(exe)]).stdout
        assert b"js_runtime_install_typedarray" not in symbols, "the typed-array group is linked"
        save("family-bytes-scratch-default", status=observed.returncode)
        # A program the detector matches by a name (`class CommandBuffer`)
        # and the fact says never creates a byte cell: the install is left
        # out and the image claims the family; with the switch off the
        # detector's install is back and nothing is claimed.
        source = fixtures / "family-bytes-detector-only.ts"
        expected = oracle(source)
        exe, e = compile_(source, "family-bytes-detector-only", PERRY_KEEP_SYMBOLS="1")
        observed = observe(exe, e)
        assert observed.returncode == 0 and observed.stdout == expected, observed
        symbols = run(["nm", str(exe)]).stdout
        assert b"js_runtime_families_never" in symbols, "the image makes the family claim"
        assert b"js_runtime_install_typedarray" not in symbols, "the typed-array group is linked"
        removed = exe.stat().st_size
        exe, e = compile_(source, "family-bytes-detector-only-nolink", PERRY_KEEP_SYMBOLS="1",
                          PERRY_FACTS_FAMILY_LINK="0")
        observed = observe(exe, e)
        assert observed.returncode == 0 and observed.stdout == expected, observed
        symbols = run(["nm", str(exe)]).stdout
        assert b"js_runtime_install_typedarray" in symbols, "the detector no longer selects the group"
        assert b"js_runtime_families_never" not in symbols, "a claim with the switch off"
        assert removed < exe.stat().st_size, (removed, exe.stat().st_size)
        save("family-bytes-detector-only", status=0, bytes_removed=exe.stat().st_size - removed)
        # `full`: the install follows the fact both ways. A program the
        # detector misses (constructors through constant computed keys) links
        # the group and answers like node.
        source = fixtures / "family-bytes-link-full.ts"
        exe, e = compile_(source, "family-bytes-link-full", PERRY_KEEP_SYMBOLS="1",
                          PERRY_FACTS_FAMILY_LINK="full")
        observed = observe(exe, e)
        assert observed.returncode == 0 and observed.stdout == oracle(source), observed
        assert b"js_runtime_install_typedarray" in run(["nm", str(exe)]).stdout
        save("family-bytes-link-full", status=observed.returncode)
    else:
        save("family-claims", status="skipped: no instrumented runtime variant")

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
        ("array-store-negative", "arrelem", "array rep"),
        ("rep-default-negative", "arrelem", "array rep"),
        ("slice-birth-negative", "slicebirth", "array rep"),
        ("inbounds-store-negative", "inbounds", "array rep"),
        # A false "no hole" claim made at its source (not at a store) is
        # reported where the for-of reads the slot.
        ("forof-hole-negative", "nohole", "array rep"),
        ("forof-dense-birth-negative", "arrdense", "array rep"),
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
        ("walk-fixed-negative", "walkfixed", "array rep"),
        ("unsigned-bound-negative", "unsignedbound", "unsigned bound"),
        ("key-sign-negative", "keysign", "key sign"),
        ("wide-key-negative", "widekey", "wide key"),
        ("literal-string-negative", "litstr", "literal string"),
        ("held-length-negative", "heldlen", "held length"),
        ("instance-literal-negative", "instlit", "array constant length"),
        ("named-length-negative", "namedlen", "named length"),
        ("quiet-sites-negative", "quiet", "quiet sites"),
        ("quiet-strings-negative", "quiet-strings", "quiet sites"),
        ("typed-array-site-negative", "tasite", "method site"),
        ("array-site-negative", "arraysite", "method site"),
        ("collection-site-negative", "collsite", "method site"),
        ("number-call-negative", "numbercall", "numeric result"),
        ("number-call-direct-negative", "numbercall", "numeric result"),
        ("field-read-call-negative", "fieldreadcall", "field read call"),
        ("array-undef-negative", "arrundef", "element floor"),
        ("const-number-negative", "constnum", "constant number"),
        ("signature-negative", "signature", "signature"),
        ("map-values-negative", "mapvals", "signature"),
        ("signature-escape-negative", "signature-escape", "signature"),
        ("signature-range-negative", "signature-range", "signature"),
        ("signed-index-negative", "signature-range", "signature"),
        ("local-range-negative", "localrange", "int32 input"),
        ("int32-read-negative", "int32read", "int32 input"),
        ("int-product-negative", "intprod", "int32 input"),
        # The int32 claim is witnessed on every binding shape: a `const`
        # read once (an F64 local with the collectors' i32 slot), and the
        # facts' I32 slot in module init, a closure and a static method.
        ("int32-const-negative", "range", "int32 input"),
        ("int32-init-negative", "range", "int32 input"),
        ("int32-closure-negative", "range", "int32 input"),
        ("undef-number-negative", "undefnum", "canonical number"),
        ("rest-field-negative", "restfield", "field rep"),
        ("rest-field-zero-negative", "restfield", "field rep"),
        ("rest-elem-negative", "restelem", "array rep"),
        ("rest-default-negative", "restelem", "array rep"),
        ("rest-raw-negative", "restraw", "array rep"),
        ("rest-lane-negative", "restlane", "field rep"),
        ("rest-lane-raw-negative", "restlaneraw", "field rep"),
        ("rest-held-negative", "restheld", "field read call"),
        ("rest-rep-negative", "restrep", "array rep"),
        ("rest-cap-negative", "restcap", "array rep"),
        ("rest-push-negative", "restpush", "array rep"),
        ("fill-birth-negative", "fillbirth", "array rep"),
        ("zero-sign-negative", "zerosign", "zero sign"),
        ("zero-sign-elem-negative", "zerosign", "zero sign"),
        ("birth-image-negative", "birthimage", "birth image"),
        ("string-call-negative", "stringcall", "call kinds"),
        ("loop-trip-negative", "looptrip", "loop trip bound"),
        # Activation reach, members on a built-in prototype (class H): each
        # arm turns one rule off, and the body the program enters through
        # the member reports "inactive body".
        ("reach-held-keys-negative", "noreachheldkeys", "inactive body"),
        ("reach-proto-accessor-negative", "noreachprotoaccessors", "inactive body"),
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
    # VR-3a: a record binding is its fields, and every other use takes a
    # materialized copy, claimed in check mode: a store into a copy reports
    # "value record mutation".
    for name, wrong_claim, predicate in [
        ("vr-ident-negative", "vrident", "value record identity"),
        ("vr-switch-negative", "vrswitch", "value record identity"),
        ("vr-inline-negative", "vrinline", "value record identity"),
        ("vr-write-negative", "vrwrite", "value record mutation"),
        # VR-3b: a record crosses a typed call as its lanes; a call that is
        # no typed call would hand a copy to the callee's boxed entry, which
        # reports "value record boxed entry".
        ("vr-untyped-negative", "vruntyped", "value record boxed entry"),
        # VR-4b: a record array is touched only by its intrinsics, whose
        # receiver holds nothing else; a body that also serves another
        # Array reports "value record array" at the intrinsic.
        ("vr-arrjoin-negative", "vrarrjoin", "value record array"),
    ]:
        source = fixtures / (name + ".ts")
        for claim in ["", wrong_claim]:
            arm = name + ("-false" if claim else "-correct")
            exe, e = compile_(source, arm, PERRY_FACTS_SABOTAGE=claim, PERRY_FACTS_VALUE_RECORDS="1")
            observed = observe(exe, e)
            if claim:
                value = report(observed)
                assert value["predicate"] == predicate, value
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
        # The INT32-boxed word is stored into an F64 array first, so with
        # array reps on (#14390) the array rep witness reports it before any
        # consumer reads it. The arm runs with them off, like the older array
        # arms, to reach the canonical-number witness at the element's
        # consumers; the default run keeps the array rep witness's report.
        exe, e = compile_(source, "canonical-number-negative-false", PERRY_RUNTIME_DIR=runtime,
                          PERRY_FACTS_ARRAY_REPS="0")
        value = report(observe(exe, e))
        assert value["predicate"] == "canonical number", value
        save("canonical-number-negative-false", status=86)
        exe, e = compile_(source, "canonical-number-negative-false-array-rep",
                          PERRY_RUNTIME_DIR=runtime)
        value = report(observe(exe, e))
        assert value["predicate"] == "array rep", value
        save("canonical-number-negative-false-array-rep", status=86)

    # P3 B4b: a birth stores a proven Number into a raw f64 lane with no
    # number test. The same runtime sabotage hands the birth an INT32-tagged
    # word; the canonical-number witness at the birth reports it, and with
    # the consumer off (`PERRY_FACTS_NUMBER_BIRTHS=0`) the birth's own test
    # sends the word to the ordinary constructor, so nothing is claimed.
    source = fixtures / "number-birth-negative.ts"
    exe, e = compile_(source, "number-birth-negative-correct")
    observed = observe(exe, e)
    assert observed.returncode == 0 and observed.stdout == oracle(source), observed
    save("number-birth-negative-correct", status=0)
    if args.canonnum_runtime:
        runtime = str(args.canonnum_runtime.resolve())
        exe, e = compile_(source, "number-birth-negative-false", PERRY_RUNTIME_DIR=runtime)
        value = report(observe(exe, e))
        assert value["predicate"] == "canonical number", value
        assert "birth lane" in value["expectation"], value
        save("number-birth-negative-false", status=86)

    # An interval rule's bounds are those of doubles (`model::range`): the
    # repro of the false int32 claim is right and unreported, and the rule
    # before it (`PERRY_FACTS_INT_BOUNDS=0`: a product past 2^60 is the empty
    # interval) is a false proof the int32 witness reports.
    source = fixtures / "int-bounds-negative.ts"
    exe, e = compile_(source, "int-bounds-negative-correct")
    observed = observe(exe, e)
    assert observed.returncode == 0 and observed.stdout == oracle(source), observed
    save("int-bounds-negative-correct", status=0)
    exe, e = compile_(source, "int-bounds-negative-false", PERRY_FACTS_INT_BOUNDS="0")
    value = report(observe(exe, e))
    assert value["predicate"] == "int32 input", value
    save("int-bounds-negative-false", status=86)

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

    # A class with reservations (#14485): a reserved key's witness admits
    # the hole at its slot behind the read's hole compare, and a key added
    # out of the layout's order through its overflow entry
    # (WPF_MAYBE_ABSENT, `slot_ref`). A real false slot claim on such a class
    # still reports: with the facts' constructor order check off
    # (`PERRY_FACTS_CTOR_ORDER=0`), `d`'s store after an out-of-order add
    # bypasses the overflow and its slot holds a key the object does not
    # name.
    source = fixtures / "slot-key-reserved.ts"
    exe, e = compile_(source, "slot-key-reserved-false", PERRY_FACTS_CTOR_ORDER="0")
    value = report(observe(exe, e))
    assert value["predicate"] == "F3 slot" and value["key"] == "d", value
    exe, e = compile_(source, "slot-key-reserved-correct")
    observed = observe(exe, e)
    assert observed.returncode == 0 and observed.stdout == oracle(source), observed
    save("slot-key-reserved", status=86, key=value["key"])

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
