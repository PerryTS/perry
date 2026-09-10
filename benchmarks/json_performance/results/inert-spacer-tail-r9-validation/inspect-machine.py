from pathlib import Path
import hashlib, json, re, subprocess

w = Path(__file__).resolve().parent
records = []
for name in ['worker', 'access-worker', 'rotating-worker', 'options']:
    a, b = (w / ('main-' + name + '.o')).read_bytes(), (w / ('candidate-' + name + '.o')).read_bytes()
    assert a == b, name
    records.append({'worker': name, 'same': True, 'sha256': hashlib.sha256(a).hexdigest()})
(w / 'worker-object-comparison.json').write_text(json.dumps(records, indent=2) + '\n')
observations = {}
for arm in ['main', 'candidate']:
    binary = w / (arm + '-options')
    cmd = ['/opt/homebrew/opt/llvm/bin/llvm-objdump', '--disassemble-symbols=_js_json_stringify_full', '--no-show-raw-insn', str(binary)]
    output = subprocess.check_output(cmd, text=True)
    (w / (arm + '-stringify-full.s')).write_text(output)
    frame = re.search(r'sub\s+sp, sp, #0x([0-9a-f]+)', output)
    entry = re.search(r'([0-9a-f]+) <_js_json_stringify_full>:', output)
    assert frame and entry
    observations[arm] = {'frame_bytes': int(frame[1], 16), 'entry': entry[1], 'command': cmd,
                         'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest()}
binary = w / 'candidate-options'
symbols = subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm', '--defined-only', str(binary)], text=True)
selected = [line.split()[-1] for line in symbols.splitlines() if 'try_inert_spacer' in line]
assert len(selected) == 1, selected
cmd = ['/opt/homebrew/opt/llvm/bin/llvm-objdump', '--disassemble-symbols=' + selected[0], '--no-show-raw-insn', str(binary)]
output = subprocess.check_output(cmd, text=True)
(w / 'candidate-inert-helper.s').write_text(output)
observations['helper'] = {'symbol': selected[0], 'command': cmd}
(w / 'machine-observations.json').write_text(json.dumps(observations, indent=2) + '\n')
print('All four worker objects are byte-identical.')
print(json.dumps(observations, indent=2))
