from pathlib import Path
import hashlib,json,subprocess
w=Path(__file__).resolve().parent;root=w.parents[3];bench=w.parents[1];read=lambda n:json.loads((w/n).read_text())
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip();base=read('base.json')['base_commit'];assert head==read('build-provenance.json')['source_commit'];assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
for arm in ['main','candidate']:
 for name,count in [('fixture',28),('options',14)]:
  rows=read(arm+'-'+name+'-validation.json');assert len(rows)==count and all(r['matches_node'] for r in rows)
 for record in read(arm+'-workers-provenance.json'):
  for path,digest in record['files'].items():assert hashlib.sha256((root/path).read_bytes()).hexdigest()==digest
assert read('root-comparison.json')['checks_match'];assert read('lazy-main-probes.json')['rows']==read('lazy-candidate-probes.json')['rows']
(w/'source.patch').write_bytes(subprocess.check_output(['git','diff',base,head],cwd=root))
(w/'provenance.json').write_text(json.dumps({'source_commit':head,'base_commit':base,'main_build':read('main-build-provenance.json'),'candidate_build':read('build-provenance.json'),'performance_measured':False,'note':'Local correctness and machine-code validation only; no R16 remote staging or measurement window.'},indent=2)+'\n')
verdict='R16 parked before performance timing: machine-code mechanism check failed. Both main and candidate retain two memcpy calls (576 bytes plus 600 bytes) and a 1376-byte capture frame. The refactor moves the 576-byte copy to default-array initialization instead of eliminating it. No CPU/RSS gain or regression is claimed. Final source has 292 JSON units, 28 Node fixtures and 14 options per arm passing; seven native findings remain unsuppressed and equal main, shadow checks pass. Exact shape-field census update passes strict/sabotage checks; lint73/74 with existing public freshness failure. Known lazy/fraction gaps remain. No PR/version bump.'
(w/'validation-verdict.txt').write_text(verdict+'\n');v='results/'+w.name+'-validation/'
report=f'''# Direct local template capture (R16)

**Parked before performance timing.** The intended machine-code reduction did not occur: main and R16 both retain a 1,376-byte capture frame and two memcpy calls. R16 moves the 576-byte intermediate copy into default-array initialization; the 600-byte final cache-publication copy remains. No R16 CPU or RSS improvement or regression was measured.

Validated source `{head}` on `codex/json-template-capture-r16`, directly based on main `{base}` (0.5.1531). The production change is confined to `remember_parse_object_template`: fill one local plan and borrow its array values during root-barrier iteration. Cache-hit construction, cache bounds and admission, root scanners, parser/string constructors and GC policies retain main’s source behavior. This branch excludes R11–R15’s other production changes.

## Machine-code result

| Observation | Main | R16 |
|---|---|---|
| Capture frame, including saved registers | 1,376 bytes | 1,376 bytes |
| 576-byte copy | Completed values array → local plan | Default values temporary → local plan |
| 600-byte copy | Local plan → thread-local cache | Local plan → thread-local cache |
| Cache-hit construction frame | 912 bytes | 912 bytes |
| parse_slow frame | 512 bytes | 512 bytes |

`inspect-capture.py` deliberately fails: it expects one copy call and a smaller frame. Its failed expectation is preserved, not relaxed. Candidate behavior validation continued separately so the source and failure remain reviewable. No performance run was launched, and no R16 files were staged on the remote benchmark host.

[Expected versus observed]({v}capture-machine-verdict.json), [main capture]({v}main-template-capture-machine.json), [candidate capture]({v}candidate-template-capture-machine.json), [checker]({v}inspect-capture.py), [cache-hit comparison]({v}cached-machine-comparison.json).

## Correctness and source inventory

- `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`: 292 pass on the final source. The new unit rejects nested-object, nested-array and oversized-array plans after a valid prefix and verifies the previous complete cache entry and retained result survive. The TypeScript fixture interleaves ineligible captures, mutation and retained results under allocation pressure.
- Main and candidate each pass 28 Node behavioral runs across auto/tape/direct parsing and normal/scheduled/full GC, plus 14 stringify-options checks. Scheduled checks assert actual moved objects and protected retired page sets. The capture fixture has 1,998 protected sets and 96,025 moved objects under auto/direct; tape has 1,999 and 95,999. Callback-only has 16 and 13,305.
- All four generated worker objects match main byte-for-byte and link their corresponding frozen runtime. Native static analysis retains seven UNSUPPRESSED findings identical to main: five unrooted globals, one unrooted string handle and one stale allocation value. Coverage is 83 functions, 2,235 statepoints and 8,418 relocates. All fourteen IR files match after removing only the first native ModuleID path comment; shadow IR requires no normalization. Both shadow variants and the ordinary/callback native checks pass. This is not an all-clean native static result.
- Final script lint: 73/74 pass; existing public benchmark input freshness fails. Compile tier and two CI-only checks are skipped. The Rust file cap passes. Full CI is not claimed.

The initial census correctly flagged the refactor’s private `ParseObjectTemplate.keys_array` declaration plus delayed assignment. The reviewed exact inventory replaces one declaration with those two sites (35→36 tracked sites), changing no ObjectHeader layout, descriptor authority, codegen site, exemption or checker rule. The updated census passes its strict authority checks and sabotage tests.

[Units]({v}unit-source.json), [behavior]({v}candidate-fixture-validation.json), [options]({v}candidate-options-validation.json), [static comparison]({v}root-comparison.json), [capture-order audit]({v}capture-audit.json), [census review]({v}shape-census-review.json), [strict census result]({v}shape-census-final.log.gz), [final lint]({v}script-lint.log.gz).

## Build provenance

The exact production command was `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`. The final build took 333.28 seconds; the compiler and both archives were frozen with verified hashes and all three mtimes after build start. Main’s original fresh build was reused by verified hashes, with workers, fixtures and IR recompiled at R16 paths.

A preliminary build from `1525b24134845943ea6306d67512f7dee499911d` completed before the census update and is preserved separately. It was never timed. Only the inventory JSON changed between that commit and the final source; the runtime embeds Git build identity, so units and the full production build were rerun from the final clean commit. Mtime-only invalidation of the two static-wrapper entrypoints and compiler entrypoint is recorded; their source bytes and build flags were unchanged.

[Final build]({v}build-provenance.json), [main reference]({v}reference-main.json), [preliminary build]({v}pre-census-build-provenance.json), [census finalization]({v}census-finalization.json), [provenance]({v}provenance.json).

## Limits and next investigation

All 24 lazy-array probes preserve main’s exit codes and full stdout. At 180 records, six zero/true-spacing cases still crash with SIGSEGV and two plain whitespace/duplicate-key cases return noncanonical raw JSON. All twelve two-record cases and the remaining four large cases pass Node. The separate main/Bun versus Node fractional-spacing difference is unchanged. These baseline gaps are not conformance passes.

[Lazy main]({v}lazy-main-probes.json), [lazy candidate]({v}lazy-candidate-probes.json), [fraction baseline]({v}fraction-baseline.json), [fraction candidate]({v}candidate-fraction.json).

No rotating, screen, full-matrix, access, short-call, stringify-options or retained-output performance qualification ran. Prepared drivers are not execution evidence. The branch has no PR or release version bump, and the full objective remains open.

The next investigation should profile the remaining full-array scan gap. Source inspection shows the lazy route can build a tape and later reparse the blob for a sequential scan. The existing general tape walker also has per-field handle scopes, dynamic property insertion and defensive temporary string copies. A batch materializer consuming validated tape may be worth investigating after phase costs are measured. This is a hypothesis, not a measured attribution or implemented R16 change; preserve sparse access, cached identities/mutations, syntax behavior and moving-GC safety.

[Read-only scan follow-up]({v}scan-followup.md), [validation manifest]({v}manifest.json). [R15 measurements](https://github.com/PerryTS/perry/blob/229b1700fbfce6745d5693d2115834d7a885bd60/benchmarks/json_performance/TEMPLATE_CALL_BOUNDARY_R15.md) document the preceding rejected runtime variant. Earlier accepted work landed through [merge train #10037](https://github.com/PerryTS/perry/pull/10037).
'''
(bench/'TEMPLATE_CAPTURE_R16.md').write_text(report);(w/'report-draft.md').write_text(report)
p=w/'archive-validation.py';p.write_text(p.read_text().replace('measured_source_commit=', 'validated_source_commit='))
print('Wrote R16 machine-code/correctness report; no performance claims.')
