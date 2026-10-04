"""Check three-arm projection with synthetic binaries and corrupted matrices."""
import copy
import hashlib
import importlib.util
import itertools
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

B = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('summary', B / 'summarize-gc-comparison.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
passed = []
mutations = {
    'valid': lambda rows: None,
    'missing cell': lambda rows: rows.pop(),
    'duplicate cell': lambda rows: rows.__setitem__(-1, copy.deepcopy(rows[0])),
    'wrong stdout': lambda rows: rows[0].update(stdout='wrong'),
    'failed cell': lambda rows: rows[0].update(rc=1),
    'wrong binary digest': lambda rows: rows[0].update(binary_sha256='a' * 64),
}
for name, mutate in mutations.items():
    with tempfile.TemporaryDirectory(prefix='gc-window-project-check-') as temp:
        root = Path(temp)
        (root / 'bin').mkdir()
        shutil.copy2(B / 'summarize-gc-comparison.py', root)
        builds = []
        rows = []
        arms = ['main69-base', 'main69-gc', 'main69-window']
        for case, arm in itertools.product(m.AUTO_CASES, arms):
            binary = root / 'bin' / f'gc-auto-synthetic-{case}-{arm}'
            binary.write_text(case + ':' + arm)
            digest = sha(binary)
            source_case = '70-documents' if case.startswith('70-documents-') else case
            builds.append(dict(case=source_case, arm=arm, rc=0, reason=None, binary_sha256=digest))
            for mode, repeat in itertools.product(['plain', 'perf'], range(3)):
                cmd = ['/usr/bin/time', '-f', 'RSS_KIB=%M', str(binary)]
                rows.append(dict(label=f'{case}-{arm}-{mode}-{repeat}', case=case, arm=arm, mode=mode, repeat=repeat, cmd=cmd,
                    rc=0, reason=None, correct=True, stdout='expected\n', binary_sha256=digest,
                    peak_rss_bytes=1024, wall_s=.1, user_s=.1, sys_s=0,
                    events={'instructions':100,'cycles':200,'minor-faults':3,'major-faults':0}))
        mutate(rows)
        for suffix, value in [('runs', rows), ('builds', builds),
                              ('oracle', {c:'expected\n' for c in m.AUTO_CASES})]:
            (root / ('gc-auto-window-v6-' + suffix + '.json')).write_text(json.dumps(value))
        (root / 'gc-runtime-window-v6-auto.exit').write_text('0\n')
        result = subprocess.run(['python3', str(B / 'project-gc-window-comparison.py'),
                                 '--root', str(root)], capture_output=True, text=True)
        if name == 'valid':
            assert result.returncode == 0, result.stderr
            proof = json.loads((root / 'gc-auto-window-v6-projection-proof.json').read_text())
            assert len(proof['projected']) == 2
            for baseline in ['main69-base', 'main69-gc']:
                summary = json.loads((root / f'gc-auto-window-v6-vs-{baseline}-summary.json').read_text())
                assert summary['complete_cells'] == 204
        else:
            assert result.returncode != 0, name
            assert not (root / 'gc-auto-window-v6-projection-proof.json').exists(), name
            passed.append(name)
proof = dict(scope=__doc__, valid_cells=306, projected_cells=[204,204],
             rejected=passed, projector_sha256=sha(B/'project-gc-window-comparison.py'),
             verifier_sha256=sha(B/'summarize-gc-comparison.py'),
             selftest_sha256=sha(Path(__file__)))
(B/'gc-window-projection-selftest.json').write_text(json.dumps(proof,indent=2)+'\n')
print('306 synthetic cells projected into two verified 204-cell comparisons; five corrupt matrices rejected.')
