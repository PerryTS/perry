from pathlib import Path
import hashlib, json, re, subprocess, sys

w = Path(__file__).resolve().parent
arm = 'main' if '--main' in sys.argv else 'candidate'
binary = w / (arm + '-rotating-worker')
llvm = Path('/opt/homebrew/opt/llvm/bin')
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
symbols = subprocess.check_output([str(llvm / 'llvm-nm'), '--defined-only', str(binary)], text=True)
selected = [line.split()[-1] for line in symbols.splitlines()
            if line.split()[-1] == '_js_json_parse' or any(s in line for s in [
                'try_reuse_parse_object_template', 'reuse_matched_object_template', 'parse_api10parse_slow'])]
assert selected
if arm == 'candidate':
    assert any('reuse_matched_object_template' in s for s in selected)
records = []
for i, symbol in enumerate(selected):
    cmd = [str(llvm / 'llvm-objdump'), '--disassemble-symbols=' + symbol,
           '--no-show-raw-insn', str(binary)]
    output = subprocess.check_output(cmd, text=True)
    dest = w / (arm + '-cached-entry-' + str(i) + '.s')
    dest.write_text(output)
    instructions = [x for x in output.splitlines() if re.match(r'\s*[0-9a-f]+:', x)]
    prologue = instructions[:12]
    subs = [int(m[1], 16) for x in prologue if (m := re.search(r'sub\s+sp, sp, #0x([0-9a-f]+)', x))]
    saves = [int(m[1], 16) for x in prologue if (m := re.search(r'\[sp, #-0x([0-9a-f]+)\]!', x))]
    records.append(dict(arm=arm, symbol=symbol, file=dest.name, command=cmd,
                        binary_sha256=sha(binary), prologue=prologue,
                        prologue_stack_bytes=sum(subs + saves)))
    print(arm, symbol, 'prologue stack bytes', sum(subs + saves), flush=True)
cmd = [str(llvm / 'llvm-objdump'), '--macho', '--indirect-symbols', str(binary)]
dest = w / (arm + '-indirect-symbols.txt')
dest.write_text(subprocess.check_output(cmd, text=True))
(w / (arm + '-cached-entry-machine.json')).write_text(json.dumps({
    'symbols': records, 'indirect_symbols': dict(command=cmd, file=dest.name, sha256=sha(dest))
}, indent=2) + '\n')
