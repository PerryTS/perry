#!/usr/bin/env python3
"""Validate recorded debugger evidence; never qualify its timing as performance."""
from pathlib import Path
import json

root = Path(__file__).resolve().parent


def read(path):
    return json.loads((root / path).read_text())


def rows(path):
    return [json.loads(line) for line in (root / path).read_text().splitlines()
            if line.startswith('{')]


summary = read('summary.json')
assert summary['diagnostic_only'] and not summary['performance_qualified']
assert len(summary['runs']) == 10
expected = {
    'observed': [(19, 6), (26, 9)],
    'collector_only': [(19, 6), (26, 9)],
    'collector_and_trigger': [(26, 9), (26, 9)],
    'minor_entry': [(19, 6), (26, 9)],
    'major_first_all': [(28, 0), (28, 0)],
}
for run in summary['runs']:
    variant, arm = run['variant'], run['arm']
    prefix = f'{variant}/results/{arm}'
    config = read(variant + '/config.json')[arm]
    result = read(prefix + '-summary.json')
    trace = rows(prefix + '.stderr')
    events = rows(prefix + '-roots.jsonl')
    assert result['exit'] == 0 and not result['errors']
    assert result['worker_sha256'] == config['sha256']
    assert result['counts']['parse'] == 27 and result['output_checked']
    assert result['output_sha256'] == summary['expected_output_sha256']
    assert any(e['event'] == 'checked_output' and e['matches_node'] for e in events)
    counts = tuple(sum(e['collection_kind'] == kind for e in trace)
                   for kind in ['full', 'minor'])
    assert counts == expected[variant][['checkpoint', 'admission'].index(arm)]
    assert counts == (run['full'], run['minor'])
    assert run['pointer_slots_read'] == sum(e['layout_scans']['pointer_slots_read']
                                           for e in trace)
    assert run['promoted_objects'] == sum(e['copying_nursery']['promoted_objects']
                                         for e in trace)
    rejected = [e for e in events if e['event'] == 'root' and e['skipped']]
    assert len(rejected) == run['root_rejections'] == result['skipped']
    assert bool(rejected) == (variant in ['collector_only', 'collector_and_trigger'])
    forced = [e for e in events if e['event'] == 'force_major']
    assert len(forced) == run['forced_major'] == (24 if variant == 'major_first_all' else 0)
    for e in forced:
        assert any('gc_safepoint_moving_minor' in f['name'] for f in e['frames'])
        assert not any('perry_fn_lifetime_worker_ts__one' in f['name'] for f in e['frames'])

for arm, after in [('checkpoint', 66_731_840), ('admission', 35_642_712)]:
    prefix = f'observed/results/{arm}'
    events = rows(prefix + '-roots.jsonl')
    trace = rows(prefix + '.stderr')
    previous = next(e for e in events if e['event'] == 'stringify_begin' and e['parse'] == 7)
    child = int.from_bytes(bytes.fromhex(previous['input']['body_hex'])[16:24], 'little')
    assert child >> 48 == 0x7ffd
    found = [e for e in events if e['event'] == 'root' and e['parse'] == 8
             and e['object']['address'] == child & 0xffffffffffff]
    assert bool(found) == (arm == 'checkpoint')
    if found:
        assert found[0]['object']['length'] == 145_000
        assert found[0]['source']['owner_frame'].endswith('gc_check_trigger')
        assert found[0]['source']['owner_frame_offset'] == 920
    assert trace[7]['arena_bytes']['before']['total_live_allocated_bytes'] == 68_699_024
    assert trace[7]['arena_bytes']['after']['total_live_allocated_bytes'] == after
    trace = rows(f'minor_entry/results/{arm}.stderr')
    minors = [e for e in trace if e['collection_kind'] == 'minor']
    assert minors[1]['copying_nursery']['promoted_objects'] == 580_000
    assert minors[1]['copying_nursery']['promoted_bytes'] == 28_992_000
    assert minors[1]['remembered_set']['dirty_slots_scanned'] == 390_760
print('Ten debugger runs, first root divergence and controlled interventions verified; no performance claim.')
