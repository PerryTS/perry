"""Recompute completed domain diagnostics from guarded raw perf logs."""
import hashlib
import importlib.util
import itertools
import json
from pathlib import Path


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1048576), b''):
            h.update(block)
    return h.hexdigest()


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def main():
    b = Path(__file__).resolve().parent
    diag = module(b / 'gc-paired-instruction-domains-v1.py', 'domains')
    stats = module(b / 'summarize-gc-comparison.py', 'stats')
    receipt = b / (diag.VERSION + '.exit')
    assert receipt.read_text().strip() == '0'
    runs_path = b / (diag.VERSION + '-runs.json')
    summary_path = b / (diag.VERSION + '-summary.json')
    runs = json.loads(runs_path.read_text())
    summary = json.loads(summary_path.read_text())
    assert len(runs) == summary['complete_cells'] == 12
    inputs = json.loads((b / (diag.VERSION + '-inputs.json')).read_text())
    assert summary['inputs'] == inputs
    for files in inputs.values():
        for p, digest in files.items():
            assert sha(Path(p)) == digest, p
    expected = {(prefix, arm, repeat) for prefix, _, arms in diag.PLANS
                for arm, repeat in itertools.product(arms, range(3))}
    seen, products, logs = set(), {}, {}
    for row in runs:
        key = tuple(row[k] for k in ['prefix', 'arm', 'repeat'])
        assert key in expected and key not in seen
        seen.add(key)
        assert row['rc'] == 0 and row['reason'] is None and row['correct'] is True
        assert row['stdout'] == row['expected_stdout']
        oracle = json.loads((b / (row['prefix'] + '-oracle.json')).read_text())
        assert row['expected_stdout'] == oracle['tscwork']
        path = Path(row['binary'])
        if str(path) not in products:
            products[str(path)] = sha(path)
        assert products[str(path)] == row['binary_sha256']
        builds = json.loads((b / (row['prefix'] + '-builds.json')).read_text())
        matches = [r for r in builds if r['arm'] == row['arm'] and r['case'] == 'tscwork']
        assert len(matches) == 1 and matches[0]['rc'] == 0
        assert matches[0]['binary_sha256'] == products[str(path)]
        assert sha(b / 'sources/real/tscwork.ts') == matches[0]['source_sha256']
        stderr = b / 'logs' / (row['label'] + '.err')
        stdout = b / 'logs' / (row['label'] + '.out')
        assert stdout.read_text() == row['expected_stdout']
        assert diag.parse_events(stderr.read_text()) == row['events']
        logs[str(stderr)] = sha(stderr)
        logs[str(stdout)] = sha(stdout)
    assert seen == expected
    recomputed = []
    for prefix, _, arms in diag.PLANS:
        metrics = {arm: {event: stats.distribution([
            r['events'][event] for r in runs if r['prefix'] == prefix and r['arm'] == arm
        ]) for event in diag.EVENTS} for arm in arms}
        changes = {}
        for event in diag.EVENTS:
            a, c = (metrics[arm][event]['median'] for arm in arms)
            changes[event] = dict(absolute=c-a, percent=100*(c/a-1) if a else None)
        recomputed.append(dict(prefix=prefix, baseline=arms[0], candidate=arms[1],
                               measurements=metrics, change=changes))
    assert summary['comparisons'] == recomputed
    result = dict(scope=summary['scope'], complete_cells=len(runs),
                  receipt_sha256=sha(receipt), runs_sha256=sha(runs_path),
                  summary_sha256=sha(summary_path), products=products, logs=logs)
    (b / (diag.VERSION + '-verification.json')).write_text(json.dumps(result, indent=2) + '\n')
    for pair in recomputed:
        print(pair['prefix'], {event: pair['change'][event] for event in diag.EVENTS})


if __name__ == '__main__':
    main()
