#!/usr/bin/env python3
"""Qualify P3 facade composition, async calls/imports and post-return children."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['wasi-sdk', 'perry', 'runtime', 'plain-runtime', 'native-runtime-dir',
                 'target-dir', 'p3-contract', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--profile', choices=['dev', 'perry-dev', 'prod'], default='perry-dev')
    parser.add_argument('--imports', action='store_true',
                        help='also qualify typed native imports and post-return children')
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    (out / 'OWNER').write_text('P3 compound async guest facade qualification; preserve.\n')
    for name, value in os.environ.items():
        if name.startswith('PERRY_GC_') and value:
            raise SystemExit('Native host GC controls must be off: ' + name)
    fixture_name = 'async-guest-imports' if args.imports else 'async-guest-exports'
    host_name = 'async_guest_imports' if args.imports else 'async_guest_exports'
    for name in ['world.wit', 'guest.ts']:
        shutil.copy2(root / 'test-wasi/components' / fixture_name / name, out / name)
    target = args.target_dir.resolve()
    env = os.environ | {'CARGO_TARGET_DIR': str(target), 'PERRY_WORKSPACE_ROOT': str(root),
        'PERRY_RUNTIME_DIR': str(args.native_runtime_dir.resolve()),
        'WASI_SDK_PATH': str(args.wasi_sdk.resolve()), 'PERRY_CODEGEN_CPU_SLOTS': '4'}
    rows = []

    def run(name, command, expected=0, extra=None):
        with (out / (name + '.log')).open('w') as log:
            status = subprocess.run(list(map(str, command)), cwd=root, env=env | (extra or {}),
                stdout=log, stderr=subprocess.STDOUT, timeout=1800).returncode
        passed = status == expected
        rows.append(dict(phase=name, exitCode=status, passed=passed))
        (out / 'results.json').write_text(json.dumps(rows, indent=2) + '\n')
        print(('PASS ' if passed else 'FAIL ') + name, flush=True)
        if not passed:
            raise SystemExit(status or 1)

    # Immutable qualification sources and checksum freshness select the actual
    # probes. Never clean packages out of a shared target pool.
    run('build', ['cargo', '-Z', 'checksum-freshness', 'build', '--locked', '--profile', args.profile,
        '-p', 'perry-wit', '--bin', 'perry-wit', '--example', 'async_guest', '--example', 'actor_compose',
        '-p', 'perry-component-host', '--example', host_name])
    profile_dir = target / ('debug' if args.profile == 'dev' else args.profile)
    suffix = '.exe' if os.name == 'nt' else ''
    frozen = out / 'binaries'
    frozen.mkdir()
    (frozen / 'OWNER').write_text('Frozen source-matched P3 async probes; preserve.\n')
    for name in ['async_guest', 'actor_compose', host_name]:
        shutil.copy2(profile_dir / 'examples' / (name + suffix), frozen / (name + suffix))
    shutil.copy2(profile_dir / ('perry-wit' + suffix), frozen / ('perry-wit' + suffix))
    host = frozen / (host_name + suffix)
    generated = out / 'generated'
    run('public-guard', [frozen / ('perry-wit' + suffix), '--guest-c', out / 'world.wit', out / 'public'], expected=1)
    assert 'async guest functions require the P3 rooted-frame adapter' in (out / 'public-guard.log').read_text()
    assert not (out / 'public').exists()
    run('manifest', [frozen / ('perry-wit' + suffix), out / 'world.wit', out / 'manifest'])
    world = json.loads((out / 'manifest/world.json').read_text())
    required = [function['name'] for interface in world['exports'] for function in interface['functions']]
    assert required and all(not interface['name'] for interface in world['exports'])
    declarations = ['--type-import=payload']
    if args.imports:
        declarations += ['--interface-import=test:async-guest-imports/service@1.0.0',
                         '--function-import=scalar', '--function-import=empty']
    run('generate', [frozen / ('async_guest' + suffix), out / 'world.wit', 'plugin', generated])
    clang = args.wasi_sdk.resolve() / 'bin' / ('clang' + suffix)
    facade = out / 'facade.component.wasm'
    run('facade-link', [clang, '--target=wasm32-wasip3', '-mexec-model=reactor', '-O1', '-g',
        '-Wall', '-Werror', '-I', generated, generated / 'perry_producer_facade.c',
        generated / 'perry_producer_forward.c', generated / 'perry_producer_facade_component_type.o',
        '-Wl,--export=cabi_realloc', '-Wl,--fatal-warnings', '-o', facade])
    marker = ('PASS: generated Perry async imports, 64 reverse pairs' if args.imports else
              'PASS: generated Perry async exports, 64 reverse wake pairs')
    components = [facade]
    libraries = []
    for kind, archive in [('plain', args.plain_runtime.resolve()), ('instrumented', args.runtime.resolve())]:
        library = out / ('libguest-' + kind + '.a')
        libraries.append(library)
        diagnostics = {'PERRY_GC_DIAG': '1', 'PERRY_GC_VERIFY_EVACUATION': '1'} if kind == 'instrumented' else {}
        run(kind + '-compile', [args.perry.resolve(), 'compile', out / 'guest.ts', '--target', 'wasip3',
            '--output-type', 'staticlib', '--no-auto-optimize', '--no-cache', '-o', library],
            extra=diagnostics | {'PERRY_RUNTIME_DIR': str(archive.parent)})
        producer = out / (kind + '.producer.component.wasm')
        # Keep the cancellation supervisor's established instrumented guest path.
        component = out / ('guest.component.wasm' if kind == 'instrumented' else 'guest-plain.component.wasm')
        components += [producer, component]
        run(kind + '-link', [clang, '--target=wasm32-wasip3', '-mexec-model=reactor', '-O1', '-g',
            '-Wall', '-Werror', '-Wno-unused-function', '-I', generated,
            generated / 'plugin.c', generated / 'perry_adapter.c', generated / 'plugin_component_type.o',
            '-Wl,--whole-archive', library, '-Wl,--no-whole-archive', archive, '-lsetjmp',
            '-Wl,--export=cabi_realloc', '-Wl,--stack-first', '-Wl,-z,stack-size=8388608',
            '-Wl,--fatal-warnings', '-o', producer])
        run(kind + '-compose', [frozen / ('actor_compose' + suffix), producer, facade,
            generated / 'perry_producer_facade.json', component])
        contract = [args.p3_contract.resolve(), '--reactor', component, *required]
        if kind == 'plain' and args.imports:
            run('reject-undeclared-functions', [*contract, *[d for d in declarations if not d.startswith('--function-import=')]], expected=1)
            assert 'non-P3 external import:' in (out / 'reject-undeclared-functions.log').read_text()
            run('reject-wrong-import-kind', [*contract, *[d for d in declarations if d != '--function-import=scalar'], '--interface-import=scalar'], expected=1)
            assert 'non-P3 external import: scalar' in (out / 'reject-wrong-import-kind.log').read_text()
            run('reject-wasi-function-allowance', [*contract, *declarations, '--function-import=wasi:cli/run@0.2.0'], expected=1)
            assert 'invalid declared application import' in (out / 'reject-wasi-function-allowance.log').read_text()
        run(kind + '-contract', [*contract, *declarations])
        log = (out / (kind + '-contract.log')).read_text()
        exports = [line.removeprefix('export ') for line in log.splitlines() if line.startswith('export ')]
        assert set(exports) == set(required), 'private scheduler exports must remain hidden'
        run(kind + '-execute', [host, out / 'world.wit', component])
        assert marker in (out / (kind + '-execute.log')).read_text()
        if kind == 'plain':
            run('plain-rejects-instrument-grant', [host, out / 'world.wit', component, '--instrument-denial'], expected=1)
            assert 'built without the GC instruments' in (out / 'plain-rejects-instrument-grant.log').read_text()
        else:
            modes = [('moving', '--moving'), ('cancel', '--cancel')]
            if args.imports:
                modes += [('failure', '--failure'), ('inline', '--inline'), ('inline-moving', '--inline-moving')]
            for name, flag in modes:
                run(name, [host, out / 'world.wit', component, flag])
                log = (out / (name + '.log')).read_text()
                if name in ['moving', 'inline-moving']:
                    copied = sum(map(int, re.findall(r'\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)', log)))
                    assert copied > 0 and 'evacuation_ok' in log
                    (out / (name + '.json')).write_text(json.dumps(dict(guestCopiedObjects=copied,
                        guestEvacuationVerified=True), indent=2) + '\n')
                if name in ['cancel', 'failure']:
                    expected = ('PASS: native pending imports cancelled with Store retirement' if args.imports else
                                'PASS: dropping a suspended generated guest call')
                    assert expected in log
                elif name.startswith('inline'):
                    assert 'PASS: ready Rust imports without embedding dispatch' in log
                else:
                    assert marker in log
    paths = [args.perry.resolve(), args.plain_runtime.resolve(), args.runtime.resolve(),
        args.p3_contract.resolve(), *components, *libraries, out / 'world.wit', out / 'guest.ts',
        *generated.iterdir(), *frozen.iterdir()]
    identities = {str(p): dict(bytes=p.stat().st_size, sha256=hashlib.sha256(p.read_bytes()).hexdigest())
        for p in paths if p.is_file()}
    (out / 'artifact-identity.json').write_text(json.dumps(identities, indent=2) + '\n')


if __name__ == '__main__':
    main()
