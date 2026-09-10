from pathlib import Path
import hashlib, json, re, subprocess, sys

w = Path(__file__).resolve().parent
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
main_only = '--main' in sys.argv
if not main_only:
    objects = []
    for name in ['worker', 'access-worker', 'rotating-worker', 'options']:
        a, b = w / ('main-' + name + '.o'), w / ('candidate-' + name + '.o')
        assert a.read_bytes() == b.read_bytes(), name
        objects.append(dict(worker=name, same=True, sha256=sha(a)))
    (w / 'worker-object-comparison.json').write_text(json.dumps(objects, indent=2) + '\n')

arm = 'main' if main_only else 'candidate'
binary = w / (arm + '-rotating-worker')
symbols = subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm', '--defined-only', str(binary)], text=True)
selected = [line.split()[-1] for line in symbols.splitlines()
            if any(s in line for s in ['parse_string_value', 'string_from_json_bytes',
                                       'string_from_json_large_bytes', 'json_source_token_utf16_len'])]
assert not any('string_from_json_large_bytes' in s or 'json_source_token_utf16_len' in s for s in selected)
records = []
for i, symbol in enumerate(selected):
    cmd = ['/opt/homebrew/opt/llvm/bin/llvm-objdump', '--disassemble-symbols=' + symbol,
           '--no-show-raw-insn', str(binary)]
    output = subprocess.check_output(cmd, text=True)
    file = w / (arm + '-parse-string-' + str(i) + '.s')
    file.write_text(output)
    frame = re.search(r'sub\s+sp, sp, #0x([0-9a-f]+)', output)
    records.append(dict(symbol=symbol, command=cmd, file=file.name, binary_sha256=sha(binary),
                        explicit_sub_frame_bytes=int(frame[1], 16) if frame else None))
    if 'parse_string_value' in symbol:
        print(arm, 'parse_string_value explicit frame:', int(frame[1], 16) if frame else 'inspect prologue')
        print('Construction calls:', '\n'.join(x for x in output.splitlines() if 'bl\t' in x and 'json_bytes' in x))
(w / (arm + '-parser-machine.json')).write_text(json.dumps(records, indent=2) + '\n')
print('Archived', arm, len(records), 'symbols; objects match main' if not main_only else 'symbols')
