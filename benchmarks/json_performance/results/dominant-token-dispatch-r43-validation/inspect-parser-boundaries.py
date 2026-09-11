from pathlib import Path
import hashlib
import json
import re
import subprocess

w = Path(__file__).resolve().parent
rows = []
for arm, binary in [
    ('r26', w / 'main-worker'),
    ('r41', w.with_name('packed-vector-escape-r41') / 'candidate-worker'),
    ('r43', w / 'candidate-worker'),
]:
    symbols = subprocess.check_output(
        ['/opt/homebrew/opt/llvm/bin/llvm-nm', '--defined-only', str(binary)], text=True
    )
    selected = [line.split()[-1] for line in symbols.splitlines()
                if '6parser' in line and ('11parse_value' in line or '20parse_object_untyped' in line)]
    assert selected, arm
    for symbol in selected:
        function = 'parse_object_untyped' if '20parse_object_untyped' in symbol else 'parse_value'
        specialization = 'true' if 'Kb1_' in symbol else 'false' if 'Kb0_' in symbol else 'direct'
        command = ['/opt/homebrew/opt/llvm/bin/llvm-objdump', '--disassemble-symbols=' + symbol,
                   '--no-show-raw-insn', str(binary)]
        raw = subprocess.check_output(command)
        instructions = [line for line in raw.decode().splitlines() if re.match(r'\s*[0-9a-f]+:', line)]
        assert instructions, symbol
        dest = w / f'{arm}-{specialization}-{function}.s'
        assert not dest.exists(), dest
        dest.write_bytes(raw)
        rows.append(dict(arm=arm, function=function, specialization=specialization, symbol=symbol,
                         instructions=len(instructions), prologue=instructions[:16], command=command,
                         output=dest.name, output_sha256=hashlib.sha256(raw).hexdigest(),
                         binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest()))
        print(arm, specialization, function, len(instructions))
(w / 'parser-boundary-disassembly.json').write_text(json.dumps(dict(
    note='Linked production code verifies the intended call boundary. Instruction counts and frame prologues are diagnostic, not performance or causal proof.',
    rows=rows), indent=2) + '\n')
