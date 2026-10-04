"""Fresh, isolated builds on the explicitly authorized temporary build host."""
from pathlib import Path
import concurrent.futures
import hashlib
import json
import os
import signal
import shutil
import subprocess
import tarfile
import time
from datetime import datetime, timezone

B = Path('/root/rss-gc-build-20261003')
E = B / 'export'
E.mkdir(exist_ok=False)
(E / 'logs').mkdir()
sha = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()
save = lambda n, v: (E / n).write_text(json.dumps(v, indent=2) + '\n')
archives = json.loads((B / 'currente532-source-archives.json').read_text())
assert all(sha(B / n) == h for n, h in archives.items())
provenance = json.loads((B / 'provenance-gc-runtime-currente532-v12.json').read_text())
assert provenance['base'] == 'e532bf3d0efce80ef699aa67b4834967050cf5ef'
assert provenance['archive_sha256'] == archives['gc-runtime-currente532-v12-source.tar.gz']
env = dict(os.environ, PATH='/root/.cargo/bin:/usr/lib/llvm-22/bin:' + os.environ['PATH'],
           LLVM_SYS_221_PREFIX='/usr/lib/llvm-22', CARGO_BUILD_JOBS='8',
           CARGO_PROFILE_RELEASE_CODEGEN_UNITS='16', CARGO_INCREMENTAL='0',
           RUST_TEST_THREADS='1')
save('inputs.json', dict(archives=archives, provenance=provenance,
     host=subprocess.check_output(['hostname'], text=True).strip(),
     boot_id=Path('/proc/sys/kernel/random/boot_id').read_text().strip(),
     rustc=subprocess.check_output(['rustc', '-Vv'], env=env, text=True),
     llvm=subprocess.check_output(['/usr/lib/llvm-22/bin/llvm-config', '--version'], text=True),
     started_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
     script_sha256=sha(__file__),
     scope='Source/product and correctness validation only; no cross-host performance comparison.'))
for arm in ['base', 'gc']:
    root = B / ('source-' + arm)
    root.mkdir(exist_ok=False)
    with tarfile.open(B / 'perry-currente532-source.tar.gz') as t:
        t.extractall(root, filter='data')
    if arm == 'gc':
        with tarfile.open(B / 'gc-runtime-currente532-v12-source.tar.gz') as t:
            t.extractall(root, filter='data')
        assert all(sha(root / p) == h for p, h in provenance['files'].items())
    assert not (B / ('target-' + arm)).exists()

def build(arm):
    root = B / ('source-' + arm)
    target = B / ('target-' + arm)
    arm_env = dict(env, CARGO_TARGET_DIR=str(target), PERRY_RUNTIME_DIR=str(target / 'release'),
                   PERRY_WORKSPACE_ROOT=str(root))
    records = []
    def run(label, args, timeout):
        start = time.monotonic()
        record = dict(label=label, command=args, source_root=str(root),
                      target_dir=str(target), runtime_dir=arm_env['PERRY_RUNTIME_DIR'])
        log = E / 'logs' / (arm + '-' + label + '.log')
        with log.open('wb') as stream:
            remaining = datetime(2026, 10, 3, 17, 40, tzinfo=timezone.utc).timestamp() - time.time()
            assert remaining > 0, 'build host export deadline reached'
            p = subprocess.Popen(args, cwd=root, env=arm_env, stdout=stream,
                                 stderr=subprocess.STDOUT, start_new_session=True)
            record['owned_process_group'] = p.pid
            try:
                record['exit_code'] = p.wait(timeout=min(timeout, remaining))
            except subprocess.TimeoutExpired:
                # Only this command's newly created process group is stopped.
                os.killpg(p.pid, signal.SIGTERM)
                try:
                    p.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    os.killpg(p.pid, signal.SIGKILL)
                    p.wait()
                record.update(exit_code=None, reason='timeout')
        record.update(elapsed_seconds=time.monotonic() - start, log_sha256=sha(log))
        records.append(record)
        save(arm + '-commands.json', records)
        assert record['exit_code'] == 0, record
    run('build', ['cargo', 'build', '--locked', '--release', '-p', 'perry',
                 '-p', 'perry-runtime-static', '-p', 'perry-stdlib-static'], 10800)
    products = E / ('currente532-' + arm)
    products.mkdir()
    for name in ['perry', 'libperry_runtime.a', 'libperry_stdlib.a']:
        shutil.copy2(target / 'release' / name, products / name)
    save(arm + '-products.json', {n.name: sha(n) for n in products.iterdir()})
    # Export products immediately, before the correctness suites finish.
    (E / (arm + '-build.exit')).write_text('0\n')
    if arm == 'gc':
        run('runtime-tests', ['cargo', 'test', '--locked', '--release', '-p',
                             'perry-runtime', '--lib'], 5400)
        run('ffi-tests', ['cargo', 'test', '--locked', '--release', '-p',
                         'perry-ffi', '--features', 'runtime-link', '--lib'], 1800)
        run('events-tests', ['cargo', 'test', '--locked', '--release', '-p',
                            'perry-ext-events', '--lib'], 1800)
    (E / (arm + '-complete.exit')).write_text('0\n')
    return arm

success = False
try:
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        futures = {pool.submit(build, a): a for a in ['base', 'gc']}
        failures = []
        for future in concurrent.futures.as_completed(futures):
            try:
                print('completed', future.result(), flush=True)
            except Exception as exc:
                failures.append(dict(arm=futures[future], error=repr(exc)))
                print('failed', futures[future], repr(exc), flush=True)
        save('failures.json', failures)
        success = not failures
finally:
    (E / 'build-driver.exit').write_text('0\n' if success else '1\n')
assert success
