from pathlib import Path
import hashlib, json, subprocess

w = Path(__file__).resolve().parent
root = w.parents[3]
original = json.loads((w/'initial-root-reuse-refusal/main-roots.json').read_text())
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
for mode in ['native','shadow']:
    a = sorted((w/('main-ir-'+mode)).glob('*.ll'))
    b = sorted((w/('candidate-ir-'+mode)).glob('*.ll'))
    assert [p.name for p in a] == [p.name for p in b] and len(a) == 9
    for old, current in zip(a,b):
        x,y = old.read_bytes(), current.read_bytes()
        if mode == 'native':
            assert x.startswith(b'; ModuleID = ') and y.startswith(b'; ModuleID = ')
            x,y = x.split(b'\n',1)[1],y.split(b'\n',1)[1]
        assert x == y,(mode,current.name)
for arm in ['main','candidate']:
    checks = []
    for old in original['checks']:
        d = w/(arm+'-ir-'+old['mode'])
        command = [str(d/Path(arg).name) if arg.endswith('.ll') else arg for arg in old['command']]
        assert command[1] == str(root/'scripts/gc_root_dominance_check.py')
        name = 'check-'+str(old['variant'])+'.log' if old['scope']=='all' else old['scope']+'-check.log'
        with (d/name).open('wb') as out:
            r = subprocess.run(command,cwd=root,stdout=out,stderr=subprocess.STDOUT)
        checks.append(old | dict(command=command,exit_code=r.returncode,log_sha256=sha(d/name)))
        print('FRESH ROOT CHECK',arm,old['mode'],old['scope'],r.returncode,flush=True)
    build = w/('frozen-main' if arm=='main' else 'frozen-build')
    record = dict(checks=checks,source_hashes=original['source_hashes'],compiler_sha256=sha(build/'perry'),
                  runtime_sha256=sha(build/'libperry_runtime.a'),passes=original['passes'],
                  checker_sha256=sha(root/'scripts/gc_root_dominance_check.py'),
                  checker_source_commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),
                  note='All five checker commands executed freshly against these exact IR files. Reference IR is preserved from frozen R26; candidate IR is emitted at current source. Native findings remain unsuppressed. Shape-census metadata changes are not handled by reusing verdicts.')
    (w/(arm+'-roots.json')).write_text(json.dumps(record,indent=2)+'\n')
    assert [(r['mode'],r['scope'],r['exit_code']) for r in checks] == [(r['mode'],r['scope'],r['exit_code']) for r in original['checks']]
