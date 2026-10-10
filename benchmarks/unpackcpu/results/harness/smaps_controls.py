from pathlib import Path
import json
import os
import subprocess
import time

lane = Path('/root/lanes/perry-unpackcpu')
while not (lane / 'profile-final-node.rc').exists():
    time.sleep(15)
assert (lane / 'profile-final-node.rc').read_text().strip() == '0'
manifest = json.loads((lane / 'bins/base/manifest.json').read_text())
programs = [p for p in manifest if p['name'] not in ['micro', 'upm']]
rows = []
env = os.environ.copy()
env['MIMALLOC_ALLOW_THP'] = '0'
env.pop('PERRY_GC_DIAG', None)
for program in programs:
    for arm in ['base', 'fix']:
        cmd = [str(lane / 'tools/no-thp'), 'taskset', '-c', '0-55', 'setarch', '-R',
               str(lane / 'bins' / arm / program['name']), *program['args']]
        snapshots = []
        with open(os.devnull, 'wb') as sink:
            child = subprocess.Popen(cmd, cwd=program['cwd'], env=env, stdout=sink, stderr=sink)
            while child.poll() is None:
                try:
                    rollup = (Path('/proc') / str(child.pid) / 'smaps_rollup').read_text()
                    status = (Path('/proc') / str(child.pid) / 'status').read_text()
                    fields = {}
                    for key in ['Rss', 'AnonHugePages', 'Anonymous', 'Private_Clean', 'Private_Dirty', 'Shared_Clean', 'Shared_Dirty']:
                        fields[key] = int(next(line.split()[1] for line in rollup.splitlines()
                                               if line.startswith(key + ':')))
                    fields['THP_enabled'] = int(next(line.split()[1] for line in status.splitlines()
                                                     if line.startswith('THP_enabled:')))
                    # Exclude the launcher before exec; it may have inherited its parent's mappings.
                    executable = os.readlink(Path('/proc') / str(child.pid) / 'exe')
                    if executable == str(lane / 'bins' / arm / program['name']):
                        snapshots.append(fields)
                except (FileNotFoundError, ProcessLookupError, StopIteration):
                    pass
                time.sleep(0.005)
        assert child.returncode == 0
        assert all(s['THP_enabled'] == 0 and s['AnonHugePages'] == 0 for s in snapshots)
        row = dict(name=program['name'], arm=arm, diagnostic_only=True, rc=child.returncode,
                   snapshots=len(snapshots), max_anon_huge_kb=max((s['AnonHugePages'] for s in snapshots), default=None),
                   max_observed_rss_kb=max((s['Rss'] for s in snapshots), default=None))
        row['peak_components_kb'] = max(snapshots, key=lambda s: s['Rss']) if snapshots else None
        rows.append(row)
        (lane / 'evidence/smaps-controls.json').write_text(json.dumps(rows, indent=2) + '\n')
        print(json.dumps(row), flush=True)
print('COMPLETE', flush=True)
