from pathlib import Path
import json

w = Path(__file__).resolve().parent
build = json.loads((w/'build-provenance.json').read_text())
main = json.loads((w/'main-build-provenance.json').read_text())
verdict = (w/'measurement-verdict.md').read_text().strip()
phases = []
for filename, key, title in [
    ('regression-recheck-analysis.json', 'focus', 'Five prior regression cases, eleven repetitions'),
    ('access-analysis.json', 'access', 'Access after parsing, seven repetitions'),
    ('full-analysis.json', 'full', 'Original 38 plus 12 consumption rows, seven repetitions'),
    ('stringify-recheck-analysis.json', 'focus', 'Three stringify rechecks, eleven repetitions'),
    ('options-analysis.json', 'options', 'Stringify options, seven repetitions'),
    ('plain-recheck-analysis.json', 'options', 'Plain stringify in the options harness, eleven-repetition recheck'),
]:
    p = w/filename
    if p.exists():
        record = json.loads(p.read_text())['phases'][key]
        phases.append((title, record))
for filename,title in [('rotating-analysis.json','Eight-input rotation, same-input and selection controls'), ('retained-analysis.json','Live retained-output comparisons')]:
    p=w/filename
    if not p.exists():continue
    record=json.loads(p.read_text());normalized=[]
    for row in record['cases']:
        r=dict(row);e=r['engines'];a=e['perry'];b=e['baseline']
        r['operation']=r.get('mode',r.get('operation',''))+((' / '+str(r['count'])+' retained') if 'count' in r else '')
        r['delta_pct']=(a['cpu_us']/b['cpu_us']-1)*100
        r['rss_delta_mib']=a['peak_rss_mib']-b['peak_rss_mib']
        r['separated_regression']=min(a['cpu_samples_us'])>max(b['cpu_samples_us'])
        r['separated_improvement']=max(a['cpu_samples_us'])<min(b['cpu_samples_us'])
        normalized.append(r)
    phases.append((title,dict(window=record['window'],timed_trials=record['trials'],cases=normalized)))
assert phases and phases[0][1]['timed_trials'] == 220
out = ['# Outlined cached-array reads: R24', '', verdict, '',
       'Earlier accepted JSON changes are merged through PR #10037, the merge train for closed #10036. This experiment is on `codex/json-outlined-cached-read-r24`, source `' + build['source_commit'] + '`. The controlled reference is independently built main `' + main['source_commit'] + '` (0.5.1531). It is a pinned reference, not a claim to have measured the latest main.', '',
       '## Change and controls', '',
       'R24 changes only R23’s `cached_read::lazy_get` annotation from `inline` to `inline(never)`. The sparse-cache and materialized dense-slot hits still bypass root construction; misses, holes and descriptors retain the original rooted fallback. There is no production GC core, policy, threshold, parse-boundary or cache-admission change.', '',
       'Main’s exact frozen build, linked workers, behavior/options checks, moving-GC receipts and native/shadow IR are reused unchanged from R23. Candidate compilation reads the same immutable R23 TypeScript paths. `main-reuse-provenance.json` records the copied file hashes and original evidence commit; copied main checks are not presented as newly executed checks.', '',
       '## Validation', '',
       '- 294 serial release JSON runtime tests passed. The exact production build emitted all three artifacts after the recorded start, and frozen copies were SHA-256 verified.',
       '- Candidate behavior, options, actual moving/protected GC, object-code and native/shadow comparisons are recorded below. Existing failures remain unsuppressed.',
       '- Local script lint passed 73 of 74 executed checks; public benchmark freshness is the existing failure. The separate file-size gate passed. The compile tier and two CI-only checks were skipped; this is not a full CI pass.', '']
for arm in ['main','candidate']:
    rows=json.loads((w/(arm+'-fixture-validation.json')).read_text())
    options=json.loads((w/(arm+'-options-validation.json')).read_text())
    assert len(rows)==46 and all(r['matches_node'] and r['exit_code']==0 for r in rows)
    assert len(options)==14 and all(r['matches_node'] for r in options)
    out.append(f"{arm}: 46 behavior and 14 option comparisons pass. {'Reused R23 reference results.' if arm=='main' else 'New R24 executions.'}")
    out.append('')
    for row in rows:
        if row['gc']=='scheduled':assert row['protected_retired_sets']>0 and row['moved_objects']>0
        if row['subject']=='test_json_cached_reads' and row['gc']=='scheduled':
            out.append(f"- Cached reads / {row['parser']}: {row['protected_retired_sets']:,} protected retired sets, {row['moved_objects']:,} moved objects.")
    out.append('')
roots=json.loads((w/'root-comparison.json').read_text());assert roots['checks_match'] and len(roots['ir_files'])==18 and all(r['same'] for r in roots['ir_files'])
out += ['All four benchmark object files match main byte-for-byte. All 18 IR files match after removing only the first native ModuleID path comment; shadow IR is byte-identical. Shadow and ordinary-worker/callback native checks pass. The full native check retains the same 16 unsuppressed main findings (15 unrooted, one stale); this is not a clean full-native verdict.', '',
        'The existing 24-case lazy-spacer matrix retains six SIGSEGV outcomes and two noncanonical outputs. The lazy-getter baseline still fails in auto/tape and passes in direct mode; the recorded fractional-spacing difference also remains. Preserving these outcomes is not a conformance pass.', '',
        '## Measurements', '',
        'Quiet M1/8 GiB host, Node 26.5.1 and Bun 1.3.14. Timed trials use fresh processes with interleaved engine order. Each terminal window was archived before any subsequent remote operation. Analyzers verify checksums/output hashes, every CPU/RSS sample and median, inputs, frozen worker hashes and the exact source patch.', '',
        'CPU is user + system time per loop iteration. Access excludes parsing; the fields loop contains three indexed reads. Parse/stringify and consumption rows include different work and must not be substituted for access latency. RSS is whole-process peak RSS, not live heap size. Separated sample ranges indicate a gain/regression in that window; overlapping ranges do not prove equivalence.', '']
for title, phase in phases:
    out += ['### '+title, '', f"Window: {phase['window']['started_utc']} to {phase['window']['finished_utc']}; {phase['timed_trials']:,} timed trials.", '',
            '| Fixture / operation | Main µs | R24 µs | Node µs | Bun µs | R24 vs main | Ranges |', '|---|---:|---:|---:|---:|---:|---|']
    for r in phase['cases']:
        e=r['engines'];state='regression' if r['separated_regression'] else 'gain' if r['separated_improvement'] else 'overlap'
        out.append('| '+r['fixture']+' / '+r['operation']+' | '+' | '.join(f"{e[a]['cpu_us']:.6f}" for a in ['baseline','perry','node','bun'])+f" | {r['delta_pct']:+.2f}% | {state} |")
    out += ['', '| Fixture / operation | Main MiB | R24 MiB | Node MiB | Bun MiB | R24 − main MiB |', '|---|---:|---:|---:|---:|---:|']
    for r in phase['cases']:
        e=r['engines'];out.append('| '+r['fixture']+' / '+r['operation']+' | '+' | '.join(f"{e[a]['peak_rss_mib']:.3f}" for a in ['baseline','perry','node','bun'])+f" | {r['rss_delta_mib']:+.3f} |")
    out.append('')
profiles=json.loads((w/'access-profiles-analysis.json').read_text())['cases']
assert len(profiles)==4 and all(r['coverage_qualified'] and r['checksum_matches_node'] for r in profiles)
out += ['## Sampled remaining cost', '', 'Four bounded one-second profiles follow a 0.5-second settling delay, with at least 500 workload samples each. These are instrumented diagnostics, separate from uninstrumented timings. Inclusive phases overlap and must not be added; zero named property-helper samples does not mean zero property-read cost, because those reads are inlined in the generated workload.', '', '| Fixture / arm | Workload samples | Root helpers | Materialized resolver | Numeric index dispatch | fmod |', '|---|---:|---:|---:|---:|---:|']
for r in profiles:
    p=r['phases'];out.append('| '+r['case']['fixture']+' / '+r['case']['mode']+f" | {r['workload_samples']} | "+' | '.join(f"{p[k]['pct_of_workload_samples']:.2f}%" for k in ['root_scope_helpers','resolve_materialized','packed_index','fmod'])+' |')
out += ['', 'The current 1 MB materialized resolver is a concrete next target; it still performs generic ownership/classification work for an explicitly live ordinary-array edge. Numeric-index validation is another measured target. The fmod share belongs to the generated access loop. A separate stringify opportunity is a fast-emitter-only path for inert zero spacing, without widening lazy raw-source copy admission or masking lazy dispatch failures. None of these follow-ups is implemented in R24.', '', 'Across the eight benchmark windows there are 3,756 timed trials, 622 verification records and 60 separate calibration trials. The four sampled workers and their four Node checksum oracles are additional diagnostics.', '']
out += ['## Build provenance', '', '`cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static` on clean committed source. Started '+build['started_utc']+f"; elapsed {build['elapsed_seconds']:.2f} seconds.", '', '| Artifact | SHA-256 |', '|---|---|']
for name,r in build['files'].items():out.append(f"| {name} | `{r['sha256']}` |")
out += ['', 'Validation, linked disassembly, build commands, raw windows, scripts and input hashes are preserved in the R24 artifact index. Performance conclusions are limited to the measured rows above.', '']
(w/'OUTLINED_CACHED_READ_R24.md').write_text('\n'.join(out))
print('WROTE',len(out),'report lines')
