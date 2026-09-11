from pathlib import Path
import hashlib, json, os, subprocess, sys

w = Path(__file__).resolve().parent
root = w.parents[3]
arm = 'candidate' if '--candidate' in sys.argv else 'main'
build = w / ('frozen-build' if arm == 'candidate' else 'frozen-main')
source = w / 'retained-zero-worker.ts'
clean = {k: v for k, v in os.environ.items() if not k.startswith('PERRY_')}
passes = subprocess.check_output(['python3', str(root / 'scripts/read_statepoint_rewrite_passes.py')], text=True).strip()
rows = []
for mode, setting in [('native', '1'), ('shadow', '0')]:
    d = w / (arm + '-retained-ir-' + mode)
    d.mkdir(exist_ok=False)
    cmd = [str(build / 'perry'), 'compile', str(source), '--no-auto-optimize', '--no-cache', '--no-link', '--trace', 'llvm', '-o', str(d / 'worker.o')]
    env = clean | {'PERRY_RUNTIME_DIR': str(build), 'PERRY_WORKSPACE_ROOT': str(root), 'PERRY_RS4GC': setting, 'PERRY_GC_MOVING_LOOP_POLLS': '1', 'PERRY_INLINE_SHADOW_SLOT': '0'}
    with (d / 'compile.log').open('wb') as log:
        subprocess.run(cmd, cwd=d, env=env, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=180)
    sources = list((d / '.perry-trace/llvm').glob('*.ll'))
    assert len(sources) == 1
    dest = d / 'zero.ll'
    if mode == 'native':
        with (d / 'rewrite.log').open('wb') as log:
            subprocess.run(['/opt/homebrew/opt/llvm/bin/opt', '-passes=' + passes, '-S', str(sources[0]), '-o', str(dest)], stdout=log, stderr=subprocess.STDOUT, check=True)
    else:
        dest.write_bytes(sources[0].read_bytes())
    variants = [['--statepoints', '--max-stale', '0', '--min-files', '1', '--min-statepoints', '1', '--min-live-bundles', '1', '--min-relocates', '1']] if mode == 'native' else [[], ['--unrooted-allocas']]
    for i, flags in enumerate(variants):
        cmd = ['python3', str(root / 'scripts/gc_root_dominance_check.py'), *flags, str(dest)]
        with (d / ('check-' + str(i) + '.log')).open('wb') as log:
            r = subprocess.run(cmd, cwd=root, stdout=log, stderr=subprocess.STDOUT)
        rows.append({'mode': mode, 'variant': i, 'exit_code': r.returncode, 'command': cmd, 'ir_sha256': hashlib.sha256(dest.read_bytes()).hexdigest()})
        print(arm, mode, i, r.returncode, flush=True)
(w / (arm + '-retained-roots.json')).write_text(json.dumps({'rows': rows, 'source_sha256': hashlib.sha256(source.read_bytes()).hexdigest(), 'compiler_sha256': hashlib.sha256((build / 'perry').read_bytes()).hexdigest(), 'runtime_sha256': hashlib.sha256((build / 'libperry_runtime.a').read_bytes()).hexdigest()}, indent=2) + '\n')
