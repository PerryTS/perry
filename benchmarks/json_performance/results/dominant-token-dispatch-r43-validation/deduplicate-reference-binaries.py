from pathlib import Path
import json
import subprocess

w = Path(__file__).resolve().parent
plan = json.loads((w / 'deduplicate-reference-plan.json').read_text())
code = 'plan = ' + repr(plan) + '\n' + r'''
from pathlib import Path
import hashlib, json, os, shutil, stat

root = Path(plan['root'])
assert root == Path('/Users/perry/json-codex-yHdsko/benchmarks/json_performance')
lock = Path.home() / 'bench.lock'
token = 'json-reference-dedup-' + str(os.getpid())
lock.mkdir()
(lock / 'owner').write_text(token)
before = shutil.disk_usage(root)._asdict()
records = []
try:
    # Preflight every candidate before changing any directory entry.
    for row in plan['rows']:
        for key in ['source', 'target']:
            p = root / row[key]
            assert p.is_file() and not p.is_symlink() and p.resolve().is_relative_to(root)
            assert p.stat().st_uid == os.getuid()
            assert hashlib.sha256(p.read_bytes()).hexdigest() == row['sha256'], str(p)
        assert stat.S_IMODE((root / row['source']).stat().st_mode) == stat.S_IMODE((root / row['target']).stat().st_mode)
    for row in plan['rows']:
        source, target = root / row['source'], root / row['target']
        previous = target.stat()
        if previous.st_ino == source.stat().st_ino:
            continue
        temporary = target.with_name('.' + target.name + '.dedup-' + str(os.getpid()))
        assert not temporary.exists()
        os.link(source, temporary)
        os.replace(temporary, target)
        assert target.stat().st_ino == source.stat().st_ino
        assert hashlib.sha256(target.read_bytes()).hexdigest() == row['sha256']
        records.append(row | {'previous_inode': previous.st_ino, 'shared_inode': target.stat().st_ino, 'logical_bytes': previous.st_size})
    print(json.dumps({'exit_code': 0, 'disk_before': before, 'disk_after': shutil.disk_usage(root)._asdict(), 'replacements': records, 'note': plan['note']}, indent=2), flush=True)
finally:
    assert (lock / 'owner').read_text() == token
    (lock / 'owner').unlink()
    lock.rmdir()
'''
(w / 'deduplicate-reference-remote.py').write_text(code)
r = subprocess.run(['ssh', 'perry@perry-macos.local', 'python3 -'], input=code.encode(), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
(w / 'deduplicate-reference.stdout').write_bytes(r.stdout)
(w / 'deduplicate-reference.stderr').write_bytes(r.stderr)
(w / 'deduplicate-reference-controller.json').write_text(json.dumps({'exit_code': r.returncode}, indent=2) + '\n')
if r.returncode:
    raise SystemExit(r.returncode)
receipt = json.loads(r.stdout)
(w / 'deduplicate-reference-result.json').write_text(json.dumps(receipt, indent=2) + '\n')
print('Verified', len(receipt['replacements']), 'identical binary replacements; free bytes', receipt['disk_before']['free'], '->', receipt['disk_after']['free'])
