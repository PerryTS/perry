"""Reproduce stale Cargo reuse with a tiny crate, then require fresh tests."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile

B = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('overlay', B / 'prepare-gc-overlay.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
with tempfile.TemporaryDirectory(prefix='perry-cargo-mtime-proof-') as tmp:
    root = Path(tmp)
    runtime = root / 'crates/perry-runtime'
    (runtime / 'src').mkdir(parents=True)
    (root / 'Cargo.toml').write_text('[workspace]\nmembers=["crates/perry-runtime"]\nresolver="2"\n')
    (runtime / 'Cargo.toml').write_text('[package]\nname="perry-runtime"\nversion="0.0.0"\nedition="2021"\n')
    (runtime / 'src/lib.rs').write_text('#[test] fn old_code_witness() { assert_eq!(1, 1); }\n')
    env = dict(os.environ, CARGO_TARGET_DIR=str(root / 'target'), CARGO_BUILD_JOBS='1')
    def run():
        result = subprocess.run(['cargo', 'test', '--offline', '--lib'], cwd=root,
                                env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        if result.returncode:
            raise RuntimeError(result.stdout)
        return result.stdout
    first = run()
    files = {'crates/perry-runtime/src/lib.rs': b'mod added;\n#[test] fn new_code_witness() { assert_eq!(added::value(), 2); }\n',
             'crates/perry-runtime/src/added.rs': b'pub fn value() -> u32 { 2 }\n'}
    archive = root / 'overlay.tar.gz'
    with tarfile.open(archive, 'w:gz') as stream:
        for name, data in files.items():
            info = tarfile.TarInfo(name)
            info.mtime = 1
            info.size = len(data)
            stream.addfile(info, io.BytesIO(data))
    manifest = root / 'manifest.json'
    manifest.write_text(json.dumps({'files': {n: hashlib.sha256(d).hexdigest() for n, d in files.items()}}))
    with tarfile.open(archive) as stream:
        stream.extractall(root, filter='data')
    stale = run()
    if 'test old_code_witness ... ok' not in stale or 'test new_code_witness ... ok' in stale:
        raise RuntimeError('stale-mtime reuse did not reproduce on this host:\n' + stale)
    cache_stamp = root / 'target/perry-auto-test/.perry-auto-build.stamp'
    cache_stamp.parent.mkdir()
    cache_stamp.write_text('content stamp attached to stale compiled bytes')
    refreshed = m.prepare(root, archive, manifest)
    if cache_stamp.exists() or len(refreshed['invalidated_auto_runtime_stamps']) != 1:
        raise RuntimeError('outer runtime cache was not invalidated')
    backup = Path(refreshed['invalidated_auto_runtime_stamps'][0]['retained_backup'])
    if backup.read_text() != 'content stamp attached to stale compiled bytes':
        raise RuntimeError('old cache-stamp evidence lost')
    fixed = run()
    if 'test new_code_witness ... ok' not in fixed or 'test old_code_witness ... ok' in fixed:
        raise RuntimeError('refresh failed to replace old compiled test:\n' + fixed)
    proof = dict(kind=__doc__, helper_sha256=m.sha(B / 'prepare-gc-overlay.py'),
                 baseline_log=first, stale_log=stale, refreshed_log=fixed,
                 source_hashes_verified=refreshed['source_hashes_verified'],
                 refreshed_inputs=refreshed['refreshed_inputs'],
                 stale_outer_stamp_retained_and_invalidated=True)
    (B / 'gc-overlay-cargo-refresh-selftest.json').write_text(json.dumps(proof, indent=2) + '\n')
print('Reproduced stale old test after extraction; source refresh runs the new test instead.')
