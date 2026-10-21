#!/usr/bin/env python3
"""Execute component tools from the final release archive outside the checkout."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tarfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {'aarch64-apple-darwin', 'x86_64-unknown-linux-gnu', 'x86_64-pc-windows-msvc'}
MARKER = 'PASS Rust component: 51 calls, typed results, callback GC, disposal'


def required(target):
    if target not in TARGETS:
        raise ValueError('Unqualified component release target: ' + target)
    if target.endswith('-msvc'):
        return ['perry.exe', 'perry-wit.exe', 'LLVM-C.dll', 'perry_runtime.lib',
                'perry_runtime.lib.gc-effects', 'perry_stdlib.lib', 'perry_component_host.lib']
    return ['perry', 'perry-wit', 'libperry_runtime.a', 'libperry_runtime.a.gc-effects',
            'libperry_stdlib.a', 'libperry_component_host.a']


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def extract(archive, destination, target):
    """Select the exact host payload; never extract arbitrary archive paths."""
    names = set(required(target))
    destination.mkdir()
    if archive.suffix == '.zip':
        reader = zipfile.ZipFile(archive)
        entries = [(item.filename, item, not item.is_dir() and
                    not stat.S_ISLNK(item.external_attr >> 16)) for item in reader.infolist()]
        open_member = reader.open
    else:
        reader = tarfile.open(archive, 'r:gz')
        entries = [(item.name, item, item.isfile()) for item in reader.getmembers()]
        open_member = reader.extractfile
    with reader:
        selected = {}
        for name, item, regular in entries:
            name = name.removeprefix('./')
            if name not in names:
                continue
            if not regular or name in selected:
                raise ValueError('Duplicate or non-regular release artifact: ' + name)
            selected[name] = item
        if set(selected) != names:
            raise ValueError('Missing component release artifacts: ' + ', '.join(sorted(names - set(selected))))
        for name, item in selected.items():
            path = destination/name
            with open_member(item) as source, path.open('wb') as output:
                shutil.copyfileobj(source, output)
            if path.stat().st_size == 0:
                raise ValueError('Empty release artifact: ' + name)
            path.chmod(0o755 if name in ('perry', 'perry-wit', 'perry.exe', 'perry-wit.exe') else 0o644)
    return {name: {'bytes': (destination/name).stat().st_size,
                   'sha256': sha(destination/name)} for name in sorted(names)}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('archive', 'componentize', 'output'):
        p.add_argument('--' + name, type=Path, required=True)
    p.add_argument('--target', required=True, choices=sorted(TARGETS))
    p.add_argument('--revision', required=True)
    args = p.parse_args()
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if revision != args.revision:
        p.error('Expected revision does not match the source checkout')
    subprocess.run(['git', 'diff', '--quiet', 'HEAD', '--'], cwd=ROOT, check=True)
    out = args.output.resolve()
    if out.exists() or out.is_relative_to(ROOT):
        p.error('--output must be a new directory outside the checkout')
    out.mkdir(parents=True)
    archive = args.archive.resolve()
    installed = out/'install'
    files = extract(archive, installed, args.target)
    fixture = out/'fixture'
    shutil.copytree(ROOT/'test-wasi/components/transform', fixture,
                    ignore=shutil.ignore_patterns('target', 'generated'))
    env = os.environ.copy()
    for name in ('PERRY_RUNTIME_DIR', 'PERRY_LIB_DIR'):
        env.pop(name, None)
    env.update(PERRY_NO_AUTO_OPTIMIZE='1', PERRY_NO_CACHE='1', PERRY_CODEGEN_CPU_SLOTS='4',
               CARGO_BUILD_JOBS='4', CARGO_INCREMENTAL='0', CARGO_TARGET_DIR=str(out/'rust-target'))
    suffix = '.exe' if args.target.endswith('-msvc') else ''
    perry, wit = installed/('perry'+suffix), installed/('perry-wit'+suffix)
    rows = []

    def run(name, command, expected=0, marker=None, cwd=fixture, timeout=600):
        result = subprocess.run([str(x) for x in command], cwd=cwd, env=env,
                                capture_output=True, timeout=timeout)
        log = out/(name+'.log')
        log.write_bytes(result.stdout + result.stderr)
        passed = result.returncode == expected and (marker is None or marker.encode() in log.read_bytes())
        rows.append({'name': name, 'exitCode': result.returncode, 'expectedExit': expected,
                     'passed': passed, 'logSha256': sha(log)})
        (out/'results.json').write_text(json.dumps(rows, indent=2)+'\n')
        if not passed:
            raise RuntimeError('Release component check failed: ' + str(log))

    run('guest', ['cargo', 'build', '--locked', '--release', '--target', 'wasm32-unknown-unknown',
        '--manifest-path', fixture/'rust/Cargo.toml'], cwd=ROOT)
    core = out/'rust-target/wasm32-unknown-unknown/release/perry_component_transform_fixture.wasm'
    component = fixture/'transform.component.wasm'
    run('componentize', [args.componentize.resolve(), core, component])
    run('bindings', [wit, fixture/'world.wit', fixture/'generated', 'plugin'])
    run('guest-bindings', [wit, '--guest-c', fixture/'world.wit', fixture/'guest-generated', 'plugin'])
    for name in ('bindings.ts', 'guest.ts', 'world.json'):
        if not (fixture/'generated'/name).is_file():
            raise RuntimeError('Packaged WIT generator omitted ' + name)
    if not list((fixture/'guest-generated').glob('*.c')):
        raise RuntimeError('Packaged WIT generator omitted canonical guest C bindings')
    engine = installed/('perry_component_host.lib' if suffix else 'libperry_component_host.a')
    withheld = out/engine.name
    engine.rename(withheld)
    try:
        plain = fixture/'plain.ts'
        plain.write_text('console.log("ordinary program without component engine");\n')
        executable = fixture/('plain'+suffix)
        run('ordinary-compile', [perry, 'compile', plain, '--no-auto-optimize', '--no-cache', '-o', executable])
        run('ordinary-run', [executable], marker='ordinary program without component engine', timeout=60)
        run('missing-engine', [perry, 'compile', fixture/'host.ts', '--no-auto-optimize', '--no-cache',
            '-o', fixture/('missing'+suffix)], expected=1, marker='perry/component requires')
    finally:
        withheld.rename(engine)
    executable = fixture/('host'+suffix)
    run('host-compile', [perry, 'compile', fixture/'host.ts', '--no-auto-optimize', '--no-cache', '-o', executable])
    run('host-run', [executable, component], marker=MARKER, timeout=60)
    receipt = {'sourceRevision': revision, 'target': args.target, 'passed': True,
        'archive': {'name': archive.name, 'sha256': sha(archive), 'bytes': archive.stat().st_size},
        'installedFiles': files, 'componentSha256': sha(component), 'hostSha256': sha(executable),
        'componentizeSha256': sha(args.componentize.resolve()), 'phases': rows,
        'scope': 'Final release archive: isolated executable-relative linking, WIT generators, typed calls, callback GC and disposal; no instrumented-GC claim'}
    (out/'qualification.json').write_text(json.dumps(receipt, indent=2)+'\n')
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
