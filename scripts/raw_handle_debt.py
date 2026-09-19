#!/usr/bin/env python3
"""Ratchet raw-pointer custody debt around GC root handles.

A `RuntimeHandleScope` gives an object liveness -- the collector marks it and
rewrites the slot. It does nothing for a raw pointer already read out of that
slot: that copy is invisible to the collector, and if the object moves it names
from-space. Every rooting bug fixed in the #7341 quarantine sweep had rooting
ALREADY; what was missing was ordering the re-read against the collection point.

`RuntimeHandle::across_{mut,const,nanbox}` expresses that ordering in one call
and never binds the pre-call address, provided the call that may allocate is
inside its closure. An empty `across_*(|| ())` refreshes across nothing and is
therefore the same debt as a bare read. `with_{mut,const}_ptr` covers the other
legitimate shape: passing the current pointer directly to a non-allocating
operation or to an entry point that establishes its own root before it can
allocate. Each bare `get_raw_*_ptr` or empty `across_*` is a site where those
contracts are a review question instead of a shape.

This is a DEBT COUNTER, not a soundness proof. Rust has no effect system to mark
"this call may allocate", so no signature can reject holding a stale copy. Not
every bare read is a bug -- many are the final read in a scope with nothing
after them. The number is meaningful because it can only be paid down.

THE RECORDED NUMBER IS ITSELF A RATCHET
=======================================

`--update` refuses to raise the baseline, but nothing made CI *run* `--update`.
A pull request could add debt sites, raise `raw_handle_debt_baseline.txt` and
the per-module ceilings to match, and the plain check would compare the new
count against the new baseline and pass. The ratchet measured the diff against
a number the same diff was allowed to move (#7659).

`--no-raise-vs <ref>` closes that: it reads both recorded files out of the pull
request's merge base and fails if the checked-out copies are larger anywhere --
the total, an existing module's ceiling, or a module that was not listed at all.
Unchanged and lower both pass, so paying debt down stays a one-step change.

RELOCATIONS: `# moved-from:` (#10583)
=====================================

Strict per-path monotonicity cannot express a pure FILE MOVE, and the 2000-line
cap (`scripts/check_file_size.sh`) forces moves regularly. Splitting a listed
module makes the bare run demand the emptied source's line be deleted (rule 3,
"matches nothing") and the destination listed -- and `--no-raise-vs` then fails
with "was not listed at the merge base" although the TOTAL never moved and the
bodies are byte-identical. #10565 only escaped it by luck: all four of that
file's sites sat in one block, so a different split carried none. A module whose
debt is spread across it could not be split at all without first paying it down.

A ledger line may therefore declare where its debt came from:

    4 crates/perry-runtime/src/object/native_module/vtable_access.rs  # moved-from: crates/perry-runtime/src/object/native_module.rs

`--no-raise-vs` then credits the destination with what the SOURCE ACTUALLY GAVE
UP between the merge base and head (`base ceiling - head ceiling`, floored at
zero), and nothing else. That keeps the ratchet monotone:

  * the total check is untouched, so the sum still cannot rise;
  * a relocation cannot launder new sites, because the credit is bounded by a
    real reduction somewhere else in the same diff;
  * two destinations splitting one source SHARE one pool -- the same surrendered
    count cannot be spent twice;
  * the annotation goes inert the moment the move lands. Once base and head
    agree about both paths the source surrenders 0, so a later raise on the
    destination is rejected exactly as before. A stale annotation is a comment,
    not a standing permit.

What it does NOT prove is that the moved bodies are the same bodies: a diff that
genuinely cleans four sites in A while adding four unrelated sites in a new B
can spell that as a relocation. Per-path monotonicity becomes total monotonicity
plus ONE declared, reviewable transfer that names its source in the diff. That
is the deliberate boundary -- a text ratchet cannot tell a move from a rewrite.

Usage:
    scripts/raw_handle_debt.py            # report, fail if above the baseline
    scripts/raw_handle_debt.py --update   # rewrite the baseline (must go DOWN)
    scripts/raw_handle_debt.py --no-raise-vs <ref>   # ...and vs. the merge base
"""
import re, subprocess, sys, pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "crates" / "perry-runtime" / "src"
BASELINE = ROOT / "scripts" / "raw_handle_debt_baseline.txt"
PAT = re.compile(
    r"\.get_raw_(?:mut|const)_ptr\b"
    r"|\.across_(?:mut|const|nanbox)"
    r"(?:\s*::\s*<[^;{}]*>)?"
    r"\s*\(\s*(?:move\s+)?\|\|\s*(?:\(\s*\)|\{\s*\})\s*\)"
)

# The accessors and scoped-pointer combinators are DEFINED here and call each
# other; counting this file would make the ratchet count its own implementation
# and rise every time a combinator is added. Exclude it.
EXCLUDE = {"crates/perry-runtime/src/gc/roots/runtime_handles.rs"}

def count():
    total, per_file = 0, {}
    for f in sorted(SRC.rglob("*.rs")):
        rel = f.relative_to(ROOT).as_posix()
        if rel in EXCLUDE:
            continue
        n = len(PAT.findall(f.read_text(encoding="utf-8", errors="replace")))
        if n:
            per_file[rel] = n
            total += n
    return total, per_file

FILES = ROOT / "scripts" / "raw_handle_debt_files.txt"

# The ONE annotation a ledger entry may carry (#10583). Anchored to the end of
# the line so it cannot be confused with a path.
MOVED_FROM = re.compile(r"#\s*moved-from:\s*(\S+)\s*$")


def parse_ledger(text):
    """`({path: ceiling}, {path: moved_from})` from the per-module file's TEXT.

    Whole-line comments and blanks are ignored. A trailing comment on an ENTRY
    must be a well-formed `# moved-from: <path>`; anything else raises. That
    strictness is the point: a typo (`moved_from:`, `moved-from :`) would
    otherwise be silently dropped as a plain comment and the relocation it was
    meant to declare would be rejected as new debt -- with a diagnostic naming
    the destination, which is the one place the author would not look.
    """
    ceilings, moves = {}, {}
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        moved = None
        if "#" in line:
            moved = MOVED_FROM.search(line)
            if not moved:
                raise SystemExit(
                    f"::error::{FILES.name}: unrecognised trailing comment on "
                    f"the ledger entry {raw.strip()!r}. The only annotation an "
                    f"entry may carry is `# moved-from: <path>` (#10583)."
                )
            line = line[: moved.start()].strip()
        n, path = line.split(None, 1)
        path = path.strip()
        ceilings[path] = int(n)
        if moved:
            moves[path] = moved.group(1)
    return ceilings, moves


def load_ceilings():
    """`{path: ceiling}` from the per-module file. Comments and blanks ignored."""
    return parse_ledger(FILES.read_text(encoding="utf-8"))[0]


def load_moves():
    """`{path: moved_from}` declared by the CHECKED-OUT per-module file."""
    return parse_ledger(FILES.read_text(encoding="utf-8"))[1]


def render_ledger(header, per_file, moves):
    """The per-module file's TEXT for `per_file`, keeping `moves` annotations.

    Separated from `--update` so the round trip through `parse_ledger` can be
    asserted: a writer that loses the annotation would revoke a relocation the
    same commit declared, and nothing else in the gate would notice.
    """
    return (
        "\n".join(header) + "\n"
        + "".join(
            f"{n} {p}" + (f"  # moved-from: {moves[p]}" if p in moves else "") + "\n"
            for p, n in sorted(per_file.items())
        )
    )


def check_per_module(per_file):
    """Per-module rules. Returns a list of human-readable violations.

    All three directions are closed, and the third is the one that makes the
    list shrink rather than drift: an entry that no longer matches anything is
    a FAILURE, exactly as in `gc_root_dominance_allowlist.json`. Without it a
    cleaned module keeps its line forever and quietly re-permits the debt the
    next time someone edits that file.
    """
    ceilings = load_ceilings()
    bad = []
    for path, n in sorted(per_file.items()):
        if path not in ceilings:
            bad.append(
                f"{path}: {n} raw-handle debt site(s) in a module with no ceiling. "
                f"New code must put real work inside RuntimeHandle::"
                f"across_{{mut,const,nanbox}} or use "
                f"with_{{mut,const}}_ptr; see #7341."
            )
        elif n > ceilings[path]:
            bad.append(
                f"{path}: raw-handle debt count {n} exceeds its ceiling of {ceilings[path]}"
            )
    for path, ceiling in sorted(ceilings.items()):
        if path not in per_file:
            bad.append(
                f"{path}: ceiling of {ceiling} matches nothing -- the module is clean "
                f"(or gone). DELETE its line so the cleanup cannot be undone."
            )
    return bad


def parse_ceilings(text):
    """`{path: ceiling}` from the per-module file's TEXT (any revision of it).

    The BASE revision's own `moved-from` annotations are deliberately dropped:
    credit is claimed by the head ledger and paid out of the base's numbers, so
    a relocation the base already recorded is just two ordinary ceilings.
    """
    return parse_ledger(text)[0]


def compare_across_base(base_total, base_ceilings, head_total, head_ceilings,
                        head_moves=None):
    """Violations for a diff that RAISES recorded debt relative to its base.

    A module absent from the base's ceilings counts as 0, so adding a line is a
    raise from zero rather than a fresh start. Removals and decreases are
    silent: the ratchet exists to stop the number going up.

    `head_moves` is `{destination: source}` from the head ledger's `moved-from:`
    annotations (#10583). A destination may be raised by at most what its source
    SURRENDERED between the base and head ledgers -- see the module docstring.
    """
    bad = []
    head_moves = head_moves or {}
    if base_total is None and not base_ceilings:
        # The merge base recorded nothing at all -- the gate did not exist yet
        # on that side. There is no number to ratchet against, so every head
        # entry would read as "newly listed". Note this is NOT the unfetchable
        # case: `git_show` refuses to resolve a bad ref rather than reporting an
        # empty one, so reaching here means the base genuinely had no records.
        return bad
    if base_total is not None and head_total > base_total:
        bad.append(
            f"baseline raised {base_total} -> {head_total} relative to the merge "
            f"base. The ratchet only goes down; convert the new sites to "
            f"RuntimeHandle::across_{{mut,const,nanbox}} / "
            f"with_{{mut,const}}_ptr instead of recording them."
        )
    # One credit pool per declared source, sized by what that source ACTUALLY
    # gave up. Two destinations naming the same source therefore share it --
    # spending the same surrendered count twice is the obvious way to launder
    # new debt through a relocation, so the pool is drained, not re-read.
    pool = {}
    for src in set(head_moves.values()):
        pool[src] = max(0, base_ceilings.get(src, 0) - head_ceilings.get(src, 0))

    for path, ceiling in sorted(head_ceilings.items()):
        was = base_ceilings.get(path, 0)
        if ceiling <= was:
            continue
        where = "was not listed" if path not in base_ceilings else f"ceiling was {was}"
        src = head_moves.get(path)
        if src is None:
            bad.append(f"{path}: ceiling raised to {ceiling} ({where} at the merge base)")
            continue
        if src == path:
            bad.append(
                f"{path}: declares `moved-from: {src}`, which is its own path. A "
                f"relocation must name the module the sites came FROM."
            )
            continue
        need = ceiling - was
        if pool[src] < need:
            bad.append(
                f"{path}: ceiling raised to {ceiling} ({where} at the merge base) "
                f"declaring `moved-from: {src}`, but {src} surrendered only "
                f"{pool[src]} site(s) between the merge base and head (needs "
                f"{need}). A relocation credits only what its source actually "
                f"gave up, so it cannot launder new debt."
            )
            continue
        pool[src] -= need
    return bad


def git_show(ref, path):
    """`<ref>:<path>`'s text, or None when that revision has no such file.

    The ref is RESOLVED FIRST, and an unresolvable one raises. That order is the
    whole point: a merge base the runner never fetched otherwise reports every
    file as absent, which reads as "the base recorded nothing" -- a comparison
    that did not happen, reported as a pass. It must be a RED build instead.
    """
    resolved = subprocess.run(
        ["git", "rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}"],
        cwd=ROOT, capture_output=True, text=True,
    )
    if resolved.returncode != 0:
        raise SystemExit(
            f"::error::cannot resolve {ref}. The merge base was not fetched, so "
            f"the raw-handle ratchet cannot compare against it -- failing rather "
            f"than passing on a comparison that did not happen. Fetch it with "
            f"`git fetch --no-tags --depth=1 origin <sha>`."
        )
    proc = subprocess.run(
        ["git", "show", f"{ref}:{path}"],
        cwd=ROOT, capture_output=True, text=True,
    )
    if proc.returncode == 0:
        return proc.stdout
    return None


def no_raise_vs(ref):
    """Fail if the CHECKED-OUT recorded debt is higher than `ref`'s."""
    base_baseline = git_show(ref, "scripts/raw_handle_debt_baseline.txt")
    base_total = int(base_baseline.split()[0]) if base_baseline else None
    base_files = git_show(ref, "scripts/raw_handle_debt_files.txt")
    base_ceilings = parse_ceilings(base_files) if base_files else {}

    head_total = int(BASELINE.read_text().split()[0])
    head_ceilings, head_moves = parse_ledger(FILES.read_text(encoding="utf-8"))

    bad = compare_across_base(base_total, base_ceilings, head_total, head_ceilings,
                              head_moves)
    if bad:
        print(f"::error::recorded raw-handle debt rose vs. {ref}: {len(bad)} violation(s)")
        for b in bad:
            print(f"  {b}")
        return 1
    relocated = ""
    if head_moves:
        relocated = (
            f", {len(head_moves)} declared relocation(s): "
            + ", ".join(f"{src} -> {dst}" for dst, src in sorted(head_moves.items()))
        )
    print(
        f"recorded debt vs. {ref}: baseline {base_total} -> {head_total}, "
        f"{len(base_ceilings)} -> {len(head_ceilings)} module ceiling(s), none raised"
        f"{relocated}"
    )
    return 0


def self_test():
    """Guard the gate against its own regressions.

    A ratchet whose matcher silently stops matching reports 0 and passes
    forever. Assert the pattern still fires on the shapes it exists to count,
    and still ignores the combinator that replaces them.
    """
    must_match = [
        "let obj = obj_h.get_raw_mut_ptr::<ObjectHeader>();",
        "src_h.get_raw_const_ptr::<u8>()",
        "h.across_mut::<ObjectHeader, _>(|| ())",
        "h.across_const::<crate::StringHeader, _>(\n    || ( )\n)",
        "h.across_nanbox(|| ())",
        "h.across_mut::<ObjectHeader, _>(move || {})",
    ]
    must_not_match = [
        "let (found, obj) = h.across_mut::<ObjectHeader, _>(|| f());",
        "h.across_const::<ObjectHeader, _>(|| g())",
        "h.across_mut::<ObjectHeader, _>(|| { mutate(); })",
        "h.with_mut_ptr::<ObjectHeader, _>(|obj| consume(obj))",
        "h.with_const_ptr::<StringHeader, _>(|key| lookup(key))",
        "h.get_nanbox_f64()",
    ]
    for line in must_match:
        if not PAT.search(line):
            print(f"self-test FAILED: pattern no longer matches: {line}")
            return 1
    for line in must_not_match:
        if PAT.search(line):
            print(f"self-test FAILED: pattern wrongly matches: {line}")
            return 1
    if not SRC.is_dir():
        print(f"self-test FAILED: source tree missing at {SRC}")
        return 1
    total, per_file = count()
    if total == 0 or not per_file:
        print("self-test FAILED: counted zero sites -- the walk is broken")
        return 1
    # The per-module rules are what make the discipline non-optional, so each
    # of the three directions is asserted rather than trusted. A rule that
    # silently stops firing is worse than no rule: it reads as "every other
    # module is locked at zero" while locking nothing.
    saved = globals()["load_ceilings"]
    globals()["load_ceilings"] = lambda: {"a.rs": 2, "gone.rs": 1}
    try:
        checks = [
            ("unlisted module carrying debt", {"a.rs": 2, "new.rs": 1}, "no ceiling"),
            ("listed module over its ceiling", {"a.rs": 3, "gone.rs": 1}, "exceeds its ceiling"),
            ("cleaned module still listed", {"a.rs": 2}, "matches nothing"),
        ]
        for label, per, needle in checks:
            if not any(needle in v for v in check_per_module(per)):
                print(f"self-test FAILED: rule did not fire: {label}")
                return 1
        if check_per_module({"a.rs": 2, "gone.rs": 1}):
            print("self-test FAILED: the clean case reported a violation")
            return 1
    finally:
        globals()["load_ceilings"] = saved

    # #7659: the merge-base rule. Its whole job is to reject a diff that moves
    # the number it is measured against, so each way of moving it is asserted
    # to fire -- and both ways of NOT moving it to stay silent, since a rule
    # that fires on an unchanged baseline would block every honest PR.
    base_ceilings = {"a.rs": 2, "b.rs": 1}
    raises = [
        ("total raised", 998, base_ceilings, 999, base_ceilings, "baseline raised"),
        ("ceiling raised", 998, base_ceilings, 998, {"a.rs": 3, "b.rs": 1}, "ceiling raised to 3"),
        ("module newly listed", 998, base_ceilings, 998,
         dict(base_ceilings, **{"c.rs": 1}), "was not listed"),
    ]
    for label, bt, bc, ht, hc, needle in raises:
        if not any(needle in v for v in compare_across_base(bt, bc, ht, hc)):
            print(f"self-test FAILED: merge-base rule did not fire: {label}")
            return 1
    holds = [
        ("unchanged", 998, base_ceilings, 998, base_ceilings),
        ("total lowered", 998, base_ceilings, 990, {"a.rs": 1}),
        ("module cleaned away", 998, base_ceilings, 997, {"a.rs": 2}),
        ("gate did not exist at the merge base", None, {}, 998, base_ceilings),
    ]
    for label, bt, bc, ht, hc in holds:
        if compare_across_base(bt, bc, ht, hc):
            print(f"self-test FAILED: merge-base rule fired on a legal diff: {label}")
            return 1

    # #10583: a pure FILE MOVE. The shape the 2000-line cap forces -- `a.rs`
    # emptied of its two sites, `split.rs` listing them, total unchanged.
    moved_base = {"a.rs": 2, "b.rs": 1}
    moved_head = {"split.rs": 2, "b.rs": 1}
    # (i) It MUST be rejected without the annotation -- otherwise the relocation
    #     support below is indistinguishable from having deleted the rule.
    if not any("was not listed" in v for v in
               compare_across_base(998, moved_base, 998, moved_head)):
        print("self-test FAILED: an UNDECLARED relocation was accepted; the "
              "per-path rule is gone, not relaxed")
        return 1
    # (ii) ...and accepted with it, because `a.rs` really did surrender two.
    declared = compare_across_base(998, moved_base, 998, moved_head,
                                   {"split.rs": "a.rs"})
    if declared:
        print(f"self-test FAILED: a declared relocation was rejected: {declared}")
        return 1
    # (iii) A relocation cannot LAUNDER new debt: `a.rs` keeps its two sites and
    #       `split.rs` claims two more anyway. (The total is held flat here so
    #       the total rule cannot be what fires -- this must be the per-path
    #       credit, or the laundering case passes the day the totals differ.)
    launder = compare_across_base(998, moved_base, 998,
                                  {"a.rs": 2, "b.rs": 1, "split.rs": 2},
                                  {"split.rs": "a.rs"})
    if not any("surrendered only 0" in v for v in launder):
        print(f"self-test FAILED: a relocation laundered new debt: {launder}")
        return 1
    # (iv) Nor may it over-draw: `a.rs` gave up one of its two, `split.rs` wants
    #      both.
    overdraw = compare_across_base(998, moved_base, 998,
                                   {"a.rs": 1, "b.rs": 1, "split.rs": 2},
                                   {"split.rs": "a.rs"})
    if not any("surrendered only 1" in v and "needs 2" in v for v in overdraw):
        print(f"self-test FAILED: a relocation over-drew its source: {overdraw}")
        return 1
    # (v) Nor may two destinations spend one source's surrender twice. A 2-site
    #     module split THREE ways is legal; claiming 2+2 out of it is not.
    three_way = compare_across_base(998, moved_base, 998,
                                    {"b.rs": 1, "x.rs": 1, "y.rs": 1},
                                    {"x.rs": "a.rs", "y.rs": "a.rs"})
    if three_way:
        print(f"self-test FAILED: a legal 1+1 split of a 2-site module was "
              f"rejected: {three_way}")
        return 1
    double = compare_across_base(998, moved_base, 998,
                                 {"b.rs": 1, "x.rs": 2, "y.rs": 2},
                                 {"x.rs": "a.rs", "y.rs": "a.rs"})
    if not any("y.rs" in v and "surrendered only 0" in v for v in double):
        print(f"self-test FAILED: one source's surrender was spent twice: {double}")
        return 1
    # (vi) A STALE annotation is inert, not a standing permit. Once the move has
    #      landed (base and head agree about both paths) the source surrenders
    #      nothing, so a later raise on the destination is rejected as before.
    landed = {"split.rs": 2, "b.rs": 1}
    stale = compare_across_base(998, landed, 998, {"split.rs": 4, "b.rs": 1},
                                {"split.rs": "a.rs"})
    if not any("split.rs" in v and "surrendered only 0" in v for v in stale):
        print(f"self-test FAILED: a stale moved-from annotation still granted "
              f"credit: {stale}")
        return 1
    # (vii) A self-referential annotation is a typo, not a relocation.
    selfmove = compare_across_base(998, moved_base, 998, {"a.rs": 3, "b.rs": 1},
                                   {"a.rs": "a.rs"})
    if not any("its own path" in v for v in selfmove):
        print(f"self-test FAILED: a self-referential relocation was not "
              f"rejected: {selfmove}")
        return 1

    # #10583, the parser. The annotation shares a line with the path, so a
    # parser that does not strip it records a ceiling for a path that does not
    # exist -- which rule 3 would then report as "matches nothing" forever.
    parsed, parsed_moves = parse_ledger(
        "# header\n"
        "2 crates/x/split.rs  # moved-from: crates/x/a.rs\n"
        "1 crates/x/b.rs\n"
    )
    if parsed != {"crates/x/split.rs": 2, "crates/x/b.rs": 1}:
        print(f"self-test FAILED: the annotation leaked into the parsed "
              f"ceilings: {parsed}")
        return 1
    if parsed_moves != {"crates/x/split.rs": "crates/x/a.rs"}:
        print(f"self-test FAILED: the annotation did not parse: {parsed_moves}")
        return 1
    # A malformed annotation must RAISE rather than read as a plain comment: a
    # silently-dropped `moved_from:` becomes "was not listed at the merge base",
    # a diagnostic that names the destination and never mentions the typo.
    for typo in ("2 x.rs  # movedfrom: a.rs\n", "2 x.rs  # see #10583\n"):
        try:
            parse_ledger(typo)
        except SystemExit:
            pass
        else:
            print(f"self-test FAILED: a malformed entry comment parsed "
                  f"silently: {typo!r}")
            return 1
    # `--update` rewrites this file wholesale; a writer that drops the
    # annotation would revoke the relocation its own commit is declaring.
    round_tripped = parse_ledger(
        render_ledger(["# header"], {"x.rs": 2, "b.rs": 1}, {"x.rs": "a.rs"})
    )
    if round_tripped != ({"x.rs": 2, "b.rs": 1}, {"x.rs": "a.rs"}):
        print(f"self-test FAILED: --update's writer loses moved-from "
              f"annotations: {round_tripped}")
        return 1
    # The failure mode this rule is most likely to die of: an unfetched merge
    # base makes every file read as absent, which is indistinguishable from
    # "the gate did not exist there" -- i.e. a silent pass. Resolving the ref
    # first is what separates them, so assert the bad ref still raises.
    try:
        git_show("0000000000000000000000000000000000000000", "scripts/raw_handle_debt_baseline.txt")
    except SystemExit:
        pass
    else:
        print("self-test FAILED: an unresolvable merge base did not fail the check")
        return 1

    print(f"self-test ok ({total} sites across {len(per_file)} files); "
          f"all three per-module rules fire, clean case silent; "
          f"merge-base rule rejects all three raises and passes four legal "
          f"diffs; relocations credit a real surrender (declared move and a "
          f"1+1 three-way split pass) and reject an undeclared move, "
          f"laundering, an over-draw, a double-spend, a stale annotation and a "
          f"self-reference; the annotation parses, survives --update's writer, "
          f"and a malformed one raises")
    return 0

def main():
    if "--self-test" in sys.argv:
        return self_test()
    if "--no-raise-vs" in sys.argv:
        i = sys.argv.index("--no-raise-vs")
        if i + 1 >= len(sys.argv) or sys.argv[i + 1].startswith("--"):
            print("--no-raise-vs needs a git ref (the pull request's merge base)")
            return 1
        return no_raise_vs(sys.argv[i + 1])
    total, per_file = count()
    if "--update" in sys.argv:
        prev = int(BASELINE.read_text().split()[0]) if BASELINE.exists() else None
        if prev is not None and total > prev:
            print(f"refusing to raise the baseline: {prev} -> {total}")
            print("the ratchet only goes down; use across_* with a real call or with_* instead")
            return 1
        BASELINE.write_text(f"{total}\n")
        # Rewrite the per-module ceilings too, preserving the header. Entries
        # that reached zero simply do not come back -- rule 3. `moved-from:`
        # annotations on surviving entries are CARRIED OVER: dropping them here
        # would silently revoke the relocation the same commit is declaring, and
        # `--no-raise-vs` would then reject the tree `--update` just wrote.
        existing_moves = load_moves()
        header = []
        for line in FILES.read_text(encoding="utf-8").splitlines():
            if line.startswith("#") or not line.strip():
                header.append(line)
            else:
                break
        FILES.write_text(render_ledger(header, per_file, existing_moves))
        print(f"baseline set to {total}" + (f" (was {prev})" if prev is not None else ""))
        print(f"per-module ceilings rewritten: {len(per_file)} entries")
        return 0
    if not BASELINE.exists():
        print(f"no baseline; run --update. current={total}")
        return 1
    prev = int(BASELINE.read_text().split()[0])
    print(f"raw-handle debt sites: {total} (baseline {prev})")

    # Per-module rules run FIRST and unconditionally. They are strictly more
    # specific than the total -- "symbol.rs exceeds its ceiling of 1" names the
    # file and the fix, where "the total rose" does not -- and if the total
    # check returned first the specific diagnostic would never be printed for
    # the most common failure (a module gaining a read).
    module_violations = check_per_module(per_file)
    if module_violations:
        print(f"::error::per-module raw-handle rules: {len(module_violations)} violation(s)")
        for b in module_violations:
            print(f"  {b}")
        print("Put the allocating call inside RuntimeHandle::across_{mut,const,nanbox}")
        print("for a post-call reload; empty `|| ()` closures are still debt.")
        print("Use with_{mut,const}_ptr for a scoped argument to a")
        print("non-allocating operation / self-rooting runtime entry point.")
        print("See #7341 and scripts/raw_handle_debt_files.txt.")
        return 1

    if total > prev:
        print(f"::error::raw-handle debt rose {prev} -> {total}")
        print("Put the allocating call inside RuntimeHandle::across_{mut,const,nanbox}")
        print("for a post-call reload; empty `|| ()` closures are still debt.")
        print("Use with_{mut,const}_ptr for a scoped argument to a")
        print("non-allocating operation / self-rooting runtime entry point.")
        print("See #7341.")
        for path, n in sorted(per_file.items(), key=lambda kv: -kv[1])[:10]:
            print(f"  {n:4d}  {path}")
        return 1
    if total < prev:
        print(f"debt fell by {prev - total}; run --update to lock it in")

    print(f"per-module: {len(per_file)} module(s) within ceilings; every other "
          f"runtime module is locked at zero")
    return 0

if __name__ == "__main__":
    sys.exit(main())
