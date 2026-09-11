from pathlib import Path
import hashlib, json, shutil, subprocess, time

w = Path(__file__).resolve().parent
root = w.parents[3]
other = Path('/Users/amlug/projects/perry/json-stringify-key-snapshot-r29')
original = other/'benchmarks/json_performance/.work/dominant-token-dispatch-r43'
head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
assert head == subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=other, text=True).strip()
assert head == '9415ce52ec36e72d643a70b0833240b407ea31d1'
for tree in [root, other]:
    assert not subprocess.check_output(['git', 'status', '--porcelain'], cwd=tree)
units = json.loads((original/'unit-source.json').read_text())
build = json.loads((original/'build-provenance.json').read_text())
assert units['source_commit'] == build['source_commit'] == head and units['exit_code'] == 0
assert json.loads((original/'build-command-result.json').read_text())['exit_code'] == 0
assert json.loads((original/'string-suite-provenance.json').read_text())['exit_code'] == 0
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
for p, digest in units['hashes'].items():
    assert sha(root/p) == sha(other/p) == digest, p
records = []
def copy(p, dest):
    assert not dest.exists(), dest
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(p, dest)
    assert sha(p) == sha(dest)
    records.append(dict(source=str(p), copy=str(dest.relative_to(root)), sha256=sha(dest)))
for name, details in build['files'].items():
    p = original/'frozen-build'/name
    assert sha(p) == details['sha256'] and p.stat().st_size == details['bytes']
    assert details['mtime'] > json.loads((original/'build-start.json').read_text())['started_unix']
    copy(p, w/'frozen-build'/name)
names = ['build-provenance.json', 'build-start.json', 'build-command-result.json', 'unit-source.json', 'unit.log', 'unit-controller-result.json', 'build.log', 'lint-results.json', 'script-lint.log', 'file-cap.log', 'worktree-provenance.json', 'lint-review.json', 'string-suite.log', 'string-suite-provenance.json', 'build-resume.json', 'build-controller.log', 'hold-production-reviewed.txt', 'format.log', 'source-push.json', 'source-push.stdout', 'source-push.stderr', 'unit-controller.log', 'build-controller-result.json', 'raw-handle-pinned-main.stdout', 'raw-handle-pinned-main.stderr', 'build-release.py', 'validate-and-build.py', 'run-lint.py']
for name in names:
    copy(original/name, w/name)
allowed = {'.py', '.json', '.log', '.stdout', '.stderr', '.ll', '.diff'}
for p in (original/'initial-controller-setup-refusal').rglob('*'):
    if p.is_file() and p.suffix in allowed and '__pycache__' not in p.parts:
        copy(p, w/'initial-controller-setup-refusal'/p.relative_to(original/'initial-controller-setup-refusal'))
(w/'build-copy-provenance.json').write_text(json.dumps(dict(source_commit=head, actual_build_worktree=str(other), validation_worktree=str(root), copied_unix=time.time(), note='Normal three-package release build copied from immutable artifacts. Both committed source trees and unit source hashes match. Canonical validation uses the original fixture source paths, so no path or anonymous-shape normalization is added. Canonical validation uses the original source paths. No alternate-worktree validation is claimed for this round.', files=records), indent=2)+'\n')
print('Copied and verified', len(records), 'frozen build, validation and supplementary payloads')
