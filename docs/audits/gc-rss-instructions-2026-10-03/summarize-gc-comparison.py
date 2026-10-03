"""Validate complete paired measurements before producing a comparison table.

This does not declare performance acceptance. It preserves ranges and signed
changes so a memory win cannot hide extra instructions or faults.
"""
import argparse
import collections
import hashlib
import itertools
import json
import math
from pathlib import Path
import statistics

AUTO_CASES = (
    '12_large_live_set', '14_grow_then_churn', '23-binary-trees',
    '30-string-build', '31-json', '51-pipeline', '60-ring-churn',
    '71-async-worker', '72-request-processing', '73-retain-then-release',
    '70-documents-replace', '70-documents-retain', '70-documents-transition',
    'tscwork', 'zodwork', '00-noop', '15-crc32',
)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1048576), b''):
            h.update(block)
    return h.hexdigest()


def validate(rows, oracles, arms, cases, fault_events):
    require(len(set(arms)) == 2, 'two distinct arms required')
    require(set(oracles) == set(cases), 'oracle case coverage differs')
    expected = set(itertools.product(cases, arms, ('plain', 'perf'), range(3)))
    seen = set()
    hashes = collections.defaultdict(set)
    for row in rows:
        key = tuple(row[k] for k in ('case', 'arm', 'mode', 'repeat'))
        require(key in expected and key not in seen, f'unexpected or duplicate cell: {key}')
        seen.add(key)
        require(row['rc'] == 0 and row['reason'] is None and row['correct'] is True,
                f'failed measurement: {key}')
        require(row['stdout'] == oracles[row['case']], f'Node output mismatch: {key}')
        digest = row['binary_sha256']
        require(len(digest) == 64 and all(c in '0123456789abcdef' for c in digest),
                f'invalid binary digest: {key}')
        hashes[row['case'], row['arm']].add(digest)
        if row['mode'] == 'plain':
            values = [row['peak_rss_bytes'], row['wall_s'], row['user_s'], row['sys_s']]
            require(isinstance(values[0], int) and values[0] > 0, f'missing RSS: {key}')
        else:
            events = {'instructions', 'cycles'} | ({'minor-faults', 'major-faults'} if fault_events else set())
            require(set(row['events']) == events, f'event coverage differs: {key}')
            values = list(row['events'].values())
            require(all(isinstance(v, int) for v in values), f'nonintegral counter: {key}')
            require(row['events']['instructions'] > 0 and row['events']['cycles'] > 0,
                    f'empty instruction measurement: {key}')
        require(all(isinstance(v, (int, float)) and math.isfinite(v) and v >= 0 for v in values),
                f'missing/nonfinite measurement: {key}')
    require(seen == expected, f'incomplete matrix: {len(seen)}/{len(expected)} cells')
    require(all(len(v) == 1 for v in hashes.values()), 'binary changed between repetitions/modes')


def distribution(values):
    return dict(min=min(values), median=statistics.median(values), max=max(values), values=values)


def validate_builds(rows, builds):
    successful = set()
    for build in builds:
        if build['rc'] != 0 or build.get('reason') is not None or not build.get('binary_sha256'):
            continue
        case = build.get('case') or Path(build['source']).stem
        successful.add((build['arm'], case, build['binary_sha256']))
    for row in rows:
        source_case = '70-documents' if row['case'].startswith('70-documents-') else row['case']
        require((row['arm'], source_case, row['binary_sha256']) in successful,
                f'measurement has no matching successful build for its source: {row["case"]}')


def summarize(rows, arms, cases, fault_events):
    results = []
    for case in cases:
        metrics = {}
        for arm in arms:
            plain = sorted((r for r in rows if r['case'] == case and r['arm'] == arm and r['mode'] == 'plain'), key=lambda r: r['repeat'])
            perf = sorted((r for r in rows if r['case'] == case and r['arm'] == arm and r['mode'] == 'perf'), key=lambda r: r['repeat'])
            metrics[arm] = {
                'rss_bytes': distribution([r['peak_rss_bytes'] for r in plain]),
                'cpu_seconds': distribution([r['user_s'] + r['sys_s'] for r in plain]),
                'wall_seconds': distribution([r['wall_s'] for r in plain]),
            }
            for event in ('instructions', 'cycles', 'minor-faults', 'major-faults'):
                if event.endswith('faults') and not fault_events:
                    continue
                metrics[arm][event] = distribution([r['events'][event] for r in perf])
        changes = {}
        for metric, baseline in metrics[arms[0]].items():
            a, b = baseline['median'], metrics[arms[1]][metric]['median']
            changes[metric] = dict(absolute=b-a, percent=100*(b/a-1) if a else None)
        results.append(dict(case=case, measurements=metrics, change=changes))
    return results


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('prefix', type=Path)
    parser.add_argument('--arms', nargs=2, required=True, metavar=('BASELINE', 'CANDIDATE'))
    parser.add_argument('--suite', choices=('auto', 'noauto'), required=True)
    parser.add_argument('--fault-events', action='store_true', help='require fault counts in an auto matrix too')
    parser.add_argument('--complete', type=Path, required=True, help='successful terminal receipt for this matrix')
    args = parser.parse_args()
    require(args.complete.read_text().strip() == '0', 'matrix has no successful terminal receipt')
    inputs = {name: Path(str(args.prefix) + f'-{name}.json') for name in ('runs', 'oracle', 'builds')}
    rows, oracles, builds = (json.loads(inputs[name].read_text()) for name in ('runs', 'oracle', 'builds'))
    cases = AUTO_CASES if args.suite == 'auto' else ('tscwork', 'zodwork')
    faults = args.suite == 'noauto' or args.fault_events
    validate(rows, oracles, args.arms, cases, faults)
    validate_builds(rows, builds)
    checked = {}
    for row in rows:
        binaries = [Path(arg) for arg in row['cmd'] if Path(arg).parent.name == 'bin' and Path(arg).name.startswith(('gc-auto-', 'gc-'))]
        require(len(binaries) == 1, f'ambiguous executable command: {row["label"]}')
        binary = binaries[0]
        if binary not in checked:
            checked[binary] = sha(binary)
        require(checked[binary] == row['binary_sha256'], f'executable changed: {binary}')
    result = dict(suite=args.suite, baseline=args.arms[0], candidate=args.arms[1],
                  complete_cells=len(rows), inputs={str(p): sha(p) for p in inputs.values()},
                  receipt=dict(path=str(args.complete), sha256=sha(args.complete)),
                  limitations='Three repetitions; shared host. Short CPU times have coarse resolution. No automatic performance-acceptance verdict.',
                  cases=summarize(rows, args.arms, cases, faults))
    out = Path(str(args.prefix) + '-summary.json')
    out.write_text(json.dumps(result, indent=2) + '\n')
    print('case                         RSS MiB baseline -> candidate   RSS %   instructions %')
    for case in result['cases']:
        a, b = (case['measurements'][arm]['rss_bytes']['median']/2**20 for arm in args.arms)
        print(f'{case["case"]:28} {a:9.3f} -> {b:9.3f} {case["change"]["rss_bytes"]["percent"]:+8.2f} {case["change"]["instructions"]["percent"]:+13.2f}')


if __name__ == '__main__':
    main()
