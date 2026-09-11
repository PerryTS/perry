from pathlib import Path
import hashlib, json, os, subprocess

work = Path(__file__).resolve().parent
root = work.parents[3]
build = work / 'frozen-build'
source = work.with_name('materialized-read-r23') / 'fraction-spacer.ts'
binary = work / 'candidate-fraction-spacer'
env = {k: v for k, v in os.environ.items() if not k.startswith('PERRY_')}
command = [str(build / 'perry'), 'compile', str(source), '--no-auto-optimize', '--no-cache', '-o', str(binary)]
with (work / 'candidate-fraction-compile.log').open('wb') as log:
    subprocess.run(command, env=env | {'PERRY_RUNTIME_DIR': str(build)}, cwd=root,
                   stdout=log, stderr=subprocess.STDOUT, check=True, timeout=180)
run = subprocess.run([str(binary)], env=env, capture_output=True, timeout=30)
(work / 'candidate-fraction.stdout').write_bytes(run.stdout)
(work / 'candidate-fraction.stderr').write_bytes(run.stderr)
baseline = json.loads((work / 'fraction-baseline.json').read_text())
expected = next(r['output'] for r in baseline['runs'] if r['engine'] == 'main')
assert run.returncode == 0 and run.stdout.decode() == expected
record = dict(command=command, exit_code=run.returncode, matches_main=True,
              note='Preserves the separately recorded main/Bun vs Node fractional-spacing difference; not a Node conformance pass.',
              files={str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
                     for p in [source, binary, build / 'perry', build / 'libperry_runtime.a', build / 'libperry_stdlib.a']})
(work / 'candidate-fraction.json').write_text(json.dumps(record, indent=2) + '\n')
print('Candidate fractional-spacing output matches the recorded hash-verified main reference.')
