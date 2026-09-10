from pathlib import Path
import hashlib, json, os, re, subprocess

root = Path(__file__).resolve().parents[4]
work = Path(__file__).resolve().parent
os.chdir(root)
clean = {k: v for k, v in os.environ.items() if not k.startswith('PERRY_')}
env = clean | {'PERRY_RUNTIME_DIR': str(work/'frozen-build'), 'PERRY_NO_AUTO_OPTIMIZE': '1'}
compiler = work/'frozen-build/perry'
commands = []

def invoke(cmd, label, env=env):
    commands.append(cmd)
    with (work/(label+'.stdout')).open('wb') as out, (work/(label+'.stderr')).open('wb') as err:
        result = subprocess.run(cmd, env=env, stdout=out, stderr=err, timeout=180)
    if result.returncode:
        raise RuntimeError((label, result.returncode, (work/(label+'.stderr')).read_text()[-4000:]))
    return (work/(label+'.stdout')).read_bytes()

inputs = [('fixture', root/'test-files/test_json_invariant_field_loop.ts'),
          ('projection', root/'test-files/test_json_scalar_projection.ts'),
          ('entry', root/'test-files/test_json_stringify_entry.ts'),
          ('access', root/'benchmarks/json_performance/access-worker.ts'),
          ('mutation', work/'mutation-probe.ts')]
for name, source in inputs:
    invoke([str(compiler), 'compile', str(source), '--no-auto-optimize', '--no-cache',
            '--trace', 'llvm', '-o', str(work/('candidate-'+name))], name+'-compile')
    print('COMPILED', name, flush=True)

stress = {'PERRY_GC_SCHEDULE_SEED': '10022', 'PERRY_GC_SCHEDULE_RATE': '0.1',
          'PERRY_GC_SCHEDULE_ALLOC_KB': '0', 'PERRY_GC_PROTECT_FROMSPACE': '1'}
matrix = []
for subject, source in inputs[:3]:
    expected = invoke(['/opt/homebrew/bin/node', '--experimental-strip-types', str(source)], 'candidate-'+subject+'-node-oracle', clean)
    for mode, settings in [('auto', {}), ('tape', {'PERRY_JSON_TAPE': '1'}), ('direct', {'PERRY_JSON_TAPE': '0'})]:
        for gc, knobs in [('normal', {}), ('scheduled', stress), ('fullgc', {'PERRY_GEN_GC': '0'})]:
            label = 'candidate-'+subject+'-'+mode+'-'+gc
            output = invoke([str(work/('candidate-'+subject))], label, clean | settings | knobs | {'PERRY_GC_DIAG': '1'})
            assert output == expected, (label, output.decode(), expected.decode())
            diagnostic = (work/(label+'.stderr')).read_text()
            protected = len(re.findall(r'\[gc-fromspace-protect\].*retired_set=#', diagnostic))
            moved = sum(sum(map(int, re.findall(r'\b(?:copied_objects|promoted_objects)=(\d+)', line)))
                        for line in diagnostic.splitlines() if line.startswith('[gc-copy-minor] ran'))
            if gc == 'scheduled':
                assert protected > 0 and moved > 0, (label, protected, moved)
            matrix.append(dict(subject=subject, mode=mode, gc=gc, matches_node=True,
                               protected_retired_sets=protected, moved_objects=moved))
            print('PASS', label, protected, moved, flush=True)
(work/'gc-witness.json').write_text(json.dumps(dict(matrix=matrix), indent=2)+'\n')

for mode in ['normal', 'index-data', 'index-getter', 'record-data', 'record-getter',
             'delete-index', 'delete-field', 'index-write', 'prototype']:
    expected = invoke(['/opt/homebrew/bin/node', '--experimental-strip-types', str(work/'mutation-probe.ts'), mode],
                      'candidate-mutation-'+mode+'-node', clean)
    actual = invoke([str(work/'candidate-mutation'), mode], 'candidate-mutation-'+mode, clean)
    assert actual == expected, (mode, actual, expected)
    print('PASS mutation', mode, flush=True)

for fixture in ['records_array_16k', 'records_array_1m', 'records_array_20m']:
    for mode in ['repeat', 'random', 'fields', 'sequential']:
        args = [str(root/'benchmarks/json_performance/.work/fixtures'/(fixture+'.json')), mode, '1000', '0']
        expected = invoke(['/opt/homebrew/bin/node', str(root/'benchmarks/json_performance/access-worker.js')]+args,
                          'candidate-access-'+fixture+'-'+mode+'-node', clean)
        actual = invoke([str(work/'candidate-access')]+args, 'candidate-access-'+fixture+'-'+mode, clean)
        def checksum(output):
            line = next(line for line in output.decode().splitlines() if line.startswith('RESULT '))
            return float(line.split()[6])
        assert checksum(actual) == checksum(expected), (fixture, mode, actual, expected)
        print('PASS access', fixture, mode, flush=True)

paths = [compiler, work/'frozen-build/libperry_runtime.a', work/'frozen-build/libperry_stdlib.a']
paths += [path for _, path in inputs] + [work/('candidate-'+name) for name, _ in inputs]
paths += [Path(__file__)]
provenance = dict(source_commit=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),
                  commands=commands, files={str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths})
(work/'validation.json').write_text(json.dumps(provenance,indent=2)+'\n')
