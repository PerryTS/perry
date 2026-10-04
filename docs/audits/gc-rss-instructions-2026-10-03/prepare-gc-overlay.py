"""Apply an owned runtime overlay without preserving stale Cargo input mtimes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import tarfile
import time


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare(root, archive, manifest):
    root = root.resolve()
    runtime = root / 'crates/perry-runtime'
    if not (runtime / 'src/lib.rs').is_file():
        raise ValueError('not a runtime source tree')
    with tarfile.open(archive) as stream:
        for member in stream.getmembers():
            if not member.isfile() or not member.name.startswith('crates/perry-runtime/'):
                raise ValueError(f'unexpected overlay member: {member.name}')
            if not (root / member.name).resolve().is_relative_to(runtime.resolve()):
                raise ValueError(f'overlay escapes runtime: {member.name}')
        stream.extractall(root, filter='data')
    files = json.loads(manifest.read_text())['files']
    for name, expected in files.items():
        path = (root / name).resolve()
        if not path.is_relative_to(root) or sha(path) != expected:
            raise ValueError(f'source identity differs: {name}')
    # Tar preserves old timestamps. Cargo can otherwise consider its newer
    # dep-info current and never discover the changed module or new child file.
    # Refresh every runtime Rust input, including feature-specific modules,
    # so the normal build AND each auto-runtime feature variant recompile.
    refreshed = sorted(runtime.rglob('*.rs'))
    before = {str(p.relative_to(root)): p.stat().st_mtime_ns for p in refreshed}
    stamp = time.time_ns()
    for path in refreshed:
        os.utime(path, ns=(stamp, stamp))
        if path.stat().st_mtime_ns != stamp:
            raise ValueError(f'failed to refresh input timestamp: {path}')
    for name, expected in files.items():
        if sha(root / name) != expected:
            raise ValueError(f'content changed during refresh: {name}')
    # The outer compiler cache may have stamped the new CONTENT hash after
    # Cargo reused an old archive. Equal contents now cannot rehabilitate
    # that stamp: force the compiler to invoke Cargo again as well.
    invalidated = []
    for cache_stamp in sorted((root / 'target').glob('perry-auto-*/.perry-auto-build.stamp')):
        backup = cache_stamp.with_name(cache_stamp.name + f'.superseded-{stamp}')
        if backup.exists():
            raise ValueError(f'cache stamp backup exists: {backup}')
        invalidated.append(dict(original=str(cache_stamp), retained_backup=str(backup),
                                sha256=sha(cache_stamp)))
        cache_stamp.rename(backup)
    return dict(root=str(root), archive_sha256=sha(archive), manifest_sha256=sha(manifest),
                refreshed_inputs=len(refreshed), refreshed_mtime_ns=stamp,
                prior_mtimes_ns=before, source_hashes_verified=True,
                invalidated_auto_runtime_stamps=invalidated)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('root', type=Path)
    parser.add_argument('archive', type=Path)
    parser.add_argument('manifest', type=Path)
    parser.add_argument('receipt', type=Path)
    args = parser.parse_args()
    result = prepare(args.root, args.archive, args.manifest)
    args.receipt.write_text(json.dumps(result, indent=2) + '\n')
    print('Refreshed', result['refreshed_inputs'], 'runtime source timestamps; content hashes verified.')
