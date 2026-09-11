from pathlib import Path
import hashlib, json, os, re, subprocess, sys

w = Path(__file__).resolve().parent
root = w.parents[3]
arm = 'candidate' if '--candidate' in sys.argv else 'main'
build = w / ('frozen-build' if arm == 'candidate' else 'frozen-main')
source = w / 'test_json_zero_spacing.ts'
clean = {k: v for k, v in os.environ.items() if not k.startswith('PERRY_')}
assert subprocess.check_output(['/opt/homebrew/bin/node', '--version'], text=True).strip() == 'v26.5.1'
node = subprocess.run(['/opt/homebrew/bin/node', '--experimental-strip-types', str(source)], env=clean, capture_output=True, timeout=180)
(w / 'zero-node.stdout').write_bytes(node.stdout)
(w / 'zero-node.stderr').write_bytes(node.stderr)
assert node.returncode == 0, node.stderr
binary = w / (arm + '-zero')
cmd = [str(build / 'perry'), 'compile', str(source), '--no-auto-optimize', '--no-cache', '-o', str(binary)]
with (w / (arm + '-zero-compile.log')).open('wb') as log:
    subprocess.run(cmd, cwd=root, env=clean | {'PERRY_RUNTIME_DIR': str(build)}, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=180)
stress = {'PERRY_GC_SCHEDULE_SEED': '10022', 'PERRY_GC_SCHEDULE_RATE': '0.1', 'PERRY_GC_SCHEDULE_ALLOC_KB': '0', 'PERRY_GC_PROTECT_FROMSPACE': '1', 'PERRY_GC_DIAG': '1'}
rows = []
for parser, settings in [('auto', {}), ('tape', {'PERRY_JSON_TAPE': '1'}), ('direct', {'PERRY_JSON_TAPE': '0'})]:
    for mode, knobs in [('normal', {}), ('scheduled', stress), ('fullgc', {'PERRY_GEN_GC': '0'})]:
        label = arm + '-zero-' + parser + '-' + mode
        r = subprocess.run([str(binary)], env=clean | settings | knobs, capture_output=True, timeout=180)
        (w / (label + '.stdout')).write_bytes(r.stdout)
        (w / (label + '.stderr')).write_bytes(r.stderr)
        diag = r.stderr.decode(errors='replace')
        protected = len(re.findall(r'\[gc-fromspace-protect\].*retired_set=#', diag))
        moved = sum(sum(map(int, re.findall(r'\b(?:copied_objects|promoted_objects)=(\d+)', line))) for line in diag.splitlines() if line.startswith('[gc-copy-minor] ran'))
        row = {'arm': arm, 'parser': parser, 'mode': mode, 'exit_code': r.returncode, 'matches_node': r.stdout == node.stdout, 'protected_retired_sets': protected, 'moved_objects': moved}
        rows.append(row)
        (w / (arm + '-zero-validation.json')).write_text(json.dumps({'command': cmd, 'rows': rows, 'files': {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in [source, binary, build / 'perry', build / 'libperry_runtime.a', build / 'libperry_stdlib.a']}}, indent=2) + '\n')
        print(label, row, flush=True)
        assert r.returncode == 0 and row['matches_node'], (label, r.stderr[-2000:])
        if mode == 'scheduled':
            assert moved > 0 and protected > 0, row
