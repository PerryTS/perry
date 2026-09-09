import argparse, ctypes, hashlib, json, os, random, resource, time
from pathlib import Path
p = argparse.ArgumentParser()
p.add_argument('--results-dir', type=Path, required=True)
a = p.parse_args()
a.results_dir.mkdir(parents=True, exist_ok=False)
root = Path(__file__).resolve().parents[2]
probe = Path(__file__).resolve().parent
lib = ctypes.CDLL(str(probe / 'libopen_scan.dylib'))
functions = {e: getattr(lib, name) for e, name in [('old','scan_old'),('new','scan_new')]}
for f in functions.values():
    f.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t]
    f.restype = ctypes.c_size_t
meta = dict(purpose='isolated opening-byte scan diagnostic, not JSON.parse acceptance',
            host=os.uname().nodename, started_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
            files={name:hashlib.sha256((probe/name).read_bytes()).hexdigest() for name in ['run.py','probe.rs','libopen_scan.dylib']},
            fixtures={})
rng = random.Random(10034)
rows = []
for fixture in ['long_string_1m','unicode_1m']:
    text = (root / '.work/fixtures' / (fixture + '.json')).read_bytes()
    data = text[1:]
    assert text[:1] in [b'{',b'[']
    expected = b'[' in data or b'{' in data
    buffer = ctypes.create_string_buffer(data)
    meta['fixtures'][fixture] = dict(sha256=hashlib.sha256(text).hexdigest(), bytes=len(data), expected=expected)
    count = 8192
    for f in functions.values():
        assert f(buffer, len(data), 8) == 8 * int(expected)
    for rep in range(9):
        order = list(functions)
        rng.shuffle(order)
        for engine in order:
            before = resource.getrusage(resource.RUSAGE_SELF)
            start = time.perf_counter_ns()
            result = functions[engine](buffer, len(data), count)
            wall = time.perf_counter_ns() - start
            after = resource.getrusage(resource.RUSAGE_SELF)
            assert result == count * int(expected)
            row = dict(fixture=fixture, engine=engine, rep=rep, count=count,
                       cpu_us=((after.ru_utime-before.ru_utime)+(after.ru_stime-before.ru_stime))*1e6/count,
                       wall_us=wall/1000/count)
            rows.append(row)
            with (a.results_dir/'timing.jsonl').open('a') as out:
                out.write(json.dumps(row)+'\n')
    print('PASS',fixture,flush=True)
meta['finished_utc']=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
(a.results_dir/'metadata.json').write_text(json.dumps(meta,indent=2)+'\n')
