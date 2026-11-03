# Raw-handle debt trend, October 2026

Baseline: `70a74e5a0d44a4ddebd0da49d43f6aef709a05fa`.
Comparator: the commit that last set the global baseline, `51a20469ef` (2026-10-05).
Scope: `scripts/raw_handle_debt.py`, `scripts/raw_handle_debt_baseline.txt`,
`scripts/raw_handle_debt_files.txt`. No gate data is changed by this document.

## Why this exists

`raw_handle_debt` is red on `main` and has been for long enough that the debt
nearly doubled while nobody could see it. The gate is not wrong — every number
below was produced by the gate itself — but a ratchet that reports the same
failure every day for a month enforces nothing. This document measures what the
failure is made of, so the next decision is made against data instead of a
1,659-line complaint.

## Headline

| Measurement | Value |
|---|---|
| Recorded global baseline | **861** sites |
| Sites at the commit that set it (`51a20469ef`) | **868** |
| Sites at the baseline above | **1,659** across 262 files |
| Drift over ~25 days | **+798** (roughly 1.9×) |
| Per-module ceilings | 107 modules, summing to **855** |
| Sites inside those 107 modules | **918** |
| Ceiling surplus across listed modules | **−63** (they are *over*, not under) |
| Files with debt that carry **no** ceiling | **155** files / **741** sites |
| Listed modules exceeding their ceiling | **14** files / **95** sites of net excess |

The gap between 855 and 861 is a pre-existing inconsistency between the global
number and the per-module ceilings; it is not part of the drift.

## The relocation hypothesis, and why it is refuted

The obvious hope is that this is module-identity drift: the 2000-line cap forces
splits, a listed module's code moves to a new path, the source keeps its line and
the destination arrives unlisted. The inventory even has a `# moved-from:`
annotation (#10583) for exactly that, so a large drift with no new code is
plausible.

The arithmetic rules it out. If debt had merely moved out of listed modules, those
modules would now sit **below** their ceilings, and that surplus would account for
the unlisted sites. It does not: the listed modules are **63 sites over** their
ceilings, and no listed path has vanished (0 of 107 are missing from the tree).

So the +798 is genuinely new: 741 sites in files that were never permitted, plus
95 sites of net excess in files that were. Relocation accounted for none of it.

## Where the debt is

Undeclared files (741 sites):

| Class | Files | Sites |
|---|---|---|
| Test files (`tests.rs`, `_tests.rs`, `/tests/`) | 62 | **348** |
| Production | 93 | **393** |

It is a long tail, not a few bad modules: the top 10 files hold 25% of the 741,
the top 25 hold 46%, the top 50 hold 68%, and it takes all 100 to reach 89%.
The largest single offender is 28 sites (`map/string_key.rs`, `object/shape_store.rs`).

The 14 listed modules over their ceiling, worst first:

```
 41/8   +33  object/field_set_by_name/fast_paths.rs
 24/7   +17  builtins/console.rs
 14/2   +12  typedarray/transform.rs
 10/4    +6  weakref.rs
  8/2    +6  object/object_ops/descriptor_helpers.rs
 24/19   +5  webassembly.rs
 16/13   +3  event_target.rs
 10/7    +3  string/concat.rs
 10/7    +3  node_stream/async_iterator.rs
  5/2    +3  object/object_ops/define_property.rs
 10/9    +1  object/field_set_by_name/tail.rs
  5/4    +1  node_submodules/mod.rs
  4/3    +1  fs/dir_glob_watch/watch.rs
  3/2    +1  json/stringify_shape_template.rs
```

## The structural reason it stayed red

Green requires **both** closed directions at once: every unlisted file at zero
(rule 1) *and* every listed file within its ceiling (rule 2). Converting the 14
over-ceiling modules — 95 sites, and the cheapest work available — leaves all 741
unlisted sites untouched and the gate exactly as red as before. Converting the top
25 unlisted files (341 sites) still leaves 400. There is no partial state with a
green report, so there is no intermediate reward for the work, and the ratchet has
no chance to re-arm until the whole 798 is gone.

That is a design consequence, not a bug: a ratchet that goes green halfway is not
a ratchet. But it means the file's own instruction — *"Lower a ceiling whenever
you convert a pair; never raise one"* — describes how an individual PR behaves
when the debt is already bounded, not what to do when the trunk has drifted 93%
with no enforcement. That case is outside what the file documents, and it needs an
explicit decision rather than another red run.

## Two executable paths

Both are one decision away; neither is a bookkeeping edit.

**A. Conversion campaign.** All 798 sites must be converted before the gate can be
green: put the reads inside `RuntimeHandle::across_{mut,const,nanbox}` or
`with_{mut,const}_ptr`. The measurable work queue is 155 unlisted files (393
production sites in 93 files are the higher-value half) plus 95 sites of excess in
14 listed modules. Cost is real GC-safety work with a test per converted site, and
no intermediate green. Sequencing by the ranked list above is what keeps it
tractable.

**B. Reset the ceilings to the measured truth, then let the ratchet bite.** The
file says of itself: *"THE POINT OF THIS FILE IS THAT IT IS NOT A TOTAL"* — yet the
only thing currently enforced at the top level is a single global 861. A reset done
properly replaces that number with per-module ceilings at today's actuals (262
entries, 1,659 total) and amends the "never raise one" paragraph to record that
this reconciliation happened and why. Consequence, honestly stated: 741 previously
forbidden sites become permitted, and 95 sites of excess become permitted. What it
buys is that from the next commit any new bare read in any of the 262 modules fails
within days instead of a month, which is the property the gate does not have today.
Without B, A has no enforcement while it runs; that is how the +798 happened.

B first, then A, is the only ordering in which each half is verifiable.

## Reproducing this

```bash
python3 scripts/raw_handle_debt.py            # 169 violations, exit 1
python3 scripts/raw_handle_debt.py --self-test

# the comparator: does the movement predate the baseline, or did the code drift?
git worktree add /tmp/base --detach 51a20469ef
(cd /tmp/base && python3 scripts/raw_handle_debt.py 2>&1 | head -1)   # 868 sites
git worktree remove /tmp/base --force
```

Per-file counts, ceilings and the listed/unlisted split come from the gate's own
`count()` and `load_ceilings()`, so no separate counter can disagree with it.

```python
import sys; sys.path.insert(0, "scripts")
import raw_handle_debt as R
total, per_file = R.count()          # (1659, {path: sites})
ceil = R.load_ceilings()             # {path: ceiling}
unlisted = {p: n for p, n in per_file.items() if p not in ceil}
```

## What this document does not do

It does not change the baseline, the ceilings, or the rule. It also does not claim
the debt is harmless: each site is a bare handle read the collector cannot see, and
`across_*` exists because that class is a real use-after-move. The measurement is
the argument for the decision, not a substitute for it.
