from pathlib import Path
import hashlib, json, re

w = Path(__file__).resolve().parent
records = []
for arm in ['main', 'candidate']:
    meta = json.loads((w / (arm + '-cached-entry-machine.json')).read_text())
    row = next(r for r in meta['symbols'] if 'object_template' in r['symbol'])
    code = (w / row['file']).read_text()
    stubs = {int(a, 16): name for a, name in re.findall(
        r'^(0x[0-9a-f]+)\s+\d+\s+(\S+)$',
        (w / (arm + '-indirect-symbols.txt')).read_text(), re.M)}
    calls = [(int(a, 16), int(b, 16)) for a, b in re.findall(
        r'^([0-9a-f]+):\s+bl\s+(0x[0-9a-f]+)', code, re.M)]
    external = [dict(instruction=hex(a), target=hex(b), symbol=stubs[b])
                for a, b in calls if b in stubs]
    records.append(dict(arm=arm, stack_bytes=row['prologue_stack_bytes'], file=row['file'],
                        sha256=hashlib.sha256(code.encode()).hexdigest(), external_calls=external))
    print(arm, 'stack', row['prologue_stack_bytes'], 'external', external, flush=True)
assert any(r['symbol'] == '_memcpy' for r in records[0]['external_calls'])
assert any(r['symbol'] == '_memcpy' for r in records[1]['external_calls'])
(w / 'cached-machine-comparison.json').write_text(json.dumps(dict(records=records), indent=2) + '\n')
