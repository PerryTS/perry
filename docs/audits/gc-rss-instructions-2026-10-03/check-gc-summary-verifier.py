"""Synthetic sabotage checks for the result verifier, not application results."""
import copy
import importlib.util
import itertools
import json
from pathlib import Path

B = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('summary', B / 'summarize-gc-comparison.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
arms = ['baseline', 'candidate']
cases = ['tscwork', 'zodwork']
oracles = {c: 'expected\n' for c in cases}


def fixture(names, faults):
    rows = []
    for case, arm, mode, rep in itertools.product(names, arms, ['plain', 'perf'], range(3)):
        events = {'instructions': 100, 'cycles': 200}
        if faults:
            events.update({'minor-faults': 3, 'major-faults': 0})
        rows.append(dict(case=case, arm=arm, mode=mode, repeat=rep, rc=0, reason=None,
                         correct=True, stdout='expected\n', binary_sha256='a'*64,
                         peak_rss_bytes=1024, wall_s=.1, user_s=.1, sys_s=0, events=events))
    return rows


rows = fixture(cases, True)
m.validate(rows, oracles, arms, cases, True)
result = m.summarize(rows, arms, cases, True)
assert result[0]['change']['major-faults'] == {'absolute': 0, 'percent': None}
for faults in (False, True):
    matrix = fixture(m.AUTO_CASES, faults)
    m.validate(matrix, {c: 'expected\n' for c in m.AUTO_CASES}, arms, m.AUTO_CASES, faults)
    assert len(matrix) == 204
mutations = {
    'missing last cell': lambda r: r.pop(),
    'duplicate replaces required cell': lambda r: r.__setitem__(-1, copy.deepcopy(r[0])),
    'wrong output despite correct flag': lambda r: r[0].update(stdout='wrong\n'),
    'failed process despite correct flag': lambda r: r[0].update(rc=1),
    'timeout despite zero status': lambda r: r[0].update(reason='timeout'),
    'binary changes between modes': lambda r: r[3].update(binary_sha256='b'*64),
    'missing RSS': lambda r: r[0].update(peak_rss_bytes=None),
    'NaN CPU': lambda r: r[0].update(user_s=float('nan')),
    'negative counter': lambda r: r[3]['events'].update(instructions=-1),
    'missing fault counter': lambda r: r[3]['events'].pop('minor-faults'),
    'zero instructions': lambda r: r[3]['events'].update(instructions=0),
}
passed = []
for name, mutate in mutations.items():
    trial = copy.deepcopy(rows)
    mutate(trial)
    try:
        m.validate(trial, oracles, arms, cases, True)
    except ValueError:
        passed.append(name)
    else:
        raise AssertionError(name)
builds = [dict(rc=0, reason=None, arm=arm, case=case, binary_sha256='a'*64)
          for case, arm in itertools.product(cases, arms)]
m.validate_builds(rows, builds)
wrong_builds = copy.deepcopy(builds)
wrong_builds[0]['case'] = 'different-workload'
try:
    m.validate_builds(rows, wrong_builds)
except ValueError:
    passed.append('build digest exists but belongs to a different workload')
else:
    raise AssertionError('cross-workload build substitution accepted')
proof = dict(kind=__doc__, valid_matrix_cells=[24, 204, 204],
             rejected_corruptions=passed, zero_baseline_percentage_remains_null=True,
             analyzer_sha256=m.sha(B / 'summarize-gc-comparison.py'),
             selftest_sha256=m.sha(Path(__file__)))
(B / 'gc-comparison-summary-selftest.json').write_text(json.dumps(proof, indent=2) + '\n')
print('Three complete synthetic matrices accepted; 12 corrupt inputs rejected; zero baseline preserved.')
