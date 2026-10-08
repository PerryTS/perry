#!/usr/bin/env python3
"""B5: zero byte-layout coupling outside buffer::store and perry-abi.

There is no debt baseline or update mode. Consumers use scoped bytes or store
accessors; emitted layout uses the shared ABI constants. Brand mutators are
forbidden even without a nearby allocation: a byte cell's brand is final.
"""
import argparse
import collections
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
STRINGS = re.compile(r'//[^\n]*|/\*.*?\*/|\'(?:\\.|[^\'\\])\'|'
                     r'r(?P<hashes>\#{0,255})".*?"(?P=hashes)|"(?:\\.|[^"\\])*"', re.S)
PATTERNS = [
    re.compile(r'size_of\s*::\s*<\s*(?:[\w:]+::)?(?:BufferHeader|TypedArrayHeader)\s*>\s*\(\s*\)'),
    re.compile(r'(?:\b(?:buf(?:fer)?(?:_data|_ptr)?|ta|view|result)\s*|\(\s*(?:buf(?:fer)?(?:_data|_ptr)?|ta|view|result)\s+as\s+\*\s*(?:const|mut)\s+u8\s*\))\.add\(\s*(?:8|16)\s*\)'),
    re.compile(r'\(\*(?:buf(?:fer)?(?:_ptr)?|ta|view|result|backing)\)\.(?:length|capacity)\s*=(?!=)'),
    re.compile(r'(?<!fn )\b(?:buffer_data(?:_mut)?|typed_array_bytes(?:_mut)?|js_value_buffer_or_typedarray_data|js_native_buffer_data_ptr)\s*\('),
    re.compile(r'\btypedarray::data_ptr(?:_mut)?\s*\('),
    re.compile(r'\b(?:js_buffer_)?mark_as_(?:array_buffer|shared_array_buffer|data_view|uint8array|secret_key|crypto_key)(?:_\w+)?\b'),
]
BYTE_EMITTERS = {
    'function.rs', 'buffer_views.rs', 'let_buffer_views.rs', 'i32_fast_path.rs',
    'inline_dyn_typed_array.rs', 'index_set_typed_array.rs', 'ta_element_read.rs',
    'typed_array_rmw.rs', 'u8_buffer_read.rs', 'arrays.rs', 'stable_packed_typed_array.rs',
}
EMITTED = re.compile(r'\b(?:add|gep)\([^;]*?"(?:8|16|10)"[^;]*?\)', re.S)
BYTE_NAME = re.compile(r'\b\w*(?:buffer|bytes|typed|view)\w*\b|\bta\b')


def inventory(root=ROOT):
    out = collections.Counter()
    for p in sorted((root / 'crates').rglob('*.rs')):
        relative = p.relative_to(root).as_posix()
        if relative.startswith('crates/perry-abi/') or relative.startswith('crates/perry-runtime/src/buffer/store'):
            continue
        source = p.read_text()
        code = STRINGS.sub(lambda m: re.sub(r'[^\n]', ' ', m.group()), source)
        matches = [m for pattern in PATTERNS for m in pattern.finditer(code)]
        # Include arbitrary names explicitly typed/cast as byte headers, then
        # follow simple pointer aliases (not array/string pointers).
        if 'BufferHeader' not in code and 'TypedArrayHeader' not in code:
            names = set()
        else:
            names = set(re.findall(r'\b(\w+)\s*:\s*\*\s*(?:mut|const)\s+(?:[\w:]+::)?(?:BufferHeader|TypedArrayHeader)\b', code))
            names.update(re.findall(r'\blet\s+(?:mut\s+)?(\w+)[^;=\n]*=\s*[^;\n]*?\bas\s*\*\s*(?:mut|const)\s+(?:[\w:]+::)?(?:BufferHeader|TypedArrayHeader)\b', code))
            names.update(re.findall(r'\blet\s+(?:mut\s+)?(\w+)[^;=]*=\s*[^;]*?\b(?:as_pointer|get_raw_mut_ptr|get_raw_const_ptr|cast)\s*::\s*<\s*(?:[\w:]+::)?(?:BufferHeader|TypedArrayHeader)\s*>', code))
        names.update(re.findall(r'\blet\s+(?:mut\s+)?(\w+)(?:\s*:\s*[^=;]+)?\s*=\s*(?:[\w:]+::)?(?:buffer_alloc|typed_array_alloc|store_alloc|js_buffer_alloc|js_uint8array_alloc|js_array_buffer_new|js_shared_array_buffer_new|new_view)\b', code))
        if names:
            alias_pairs = re.findall(r'\blet\s+(?:mut\s+)?(\w+)\s*=\s*\(?\s*(\w+)\s*(?:as\s+\*\s*(?:const|mut)\s+u8)?\s*\)?\s*;', code)
            alias_pairs += re.findall(r'\blet\s+(?:mut\s+)?(\w+)\s*=\s*(\w+)\s*\.(?:cast(?:\s*::\s*<\s*u8\s*>)?|cast_mut|cast_const)\s*\(\s*\)\s*;', code)
            while True:
                aliases = {alias for alias, src in alias_pairs if src in names}
                if aliases <= names: break
                names.update(aliases)
        if names:
            receiver = r'(?:' + '|'.join(re.escape(n) for n in sorted(names)) + ')'
            matches.extend(re.finditer(r'(?:\b'+receiver+r'\s*|\(\s*'+receiver+r'\s+as\s+\*\s*(?:const|mut)\s+u8\s*\))\.add\(\s*(?:8|16)\s*\)', code))
            matches.extend(re.finditer(r'\(\*\s*'+receiver+r'\s*\)\s*\.(?:length|capacity)\s*=(?!=)', code))
        if '/perry-codegen/' in relative:
            for m in EMITTED.finditer(source):
                # An operation inside a comment/string fixture isn't code.
                if not re.search(r'\b(?:add|gep)\s*\(', code[m.start():m.start()+4]): continue
                if p.name in BYTE_EMITTERS or BYTE_NAME.search(m.group()): matches.append(m)
        lines = source.splitlines()
        for line in sorted({source.count('\n', 0, m.start()) for m in matches}):
            out[f'{relative}:{line+1}|{lines[line].strip()}'] += 1
    return out


def self_test():
    # Each fixture independently runs the executable gate and must exit red.
    fixtures = [
        'let dst = (buf as *mut u8).add(8);',
        'let dst = (buffer as *mut u8).add(16);',
        '(*buffer).length = 8;', '(*ta).capacity = 16;',
        'std::mem::size_of::<crate::buffer::BufferHeader>()',
        'std::mem::size_of::<TypedArrayHeader>()',
        'let n = std::mem::size_of::<\n crate::typedarray::TypedArrayHeader\n>();',
        'fn f(p: *mut BufferHeader) { (*p).length = 1; }',
        'let p = crate::buffer::buffer_alloc(8); (*p).capacity = 4;',
        'let p = typed_array_alloc(1, 8); let a = p; let b = a; let c = b; let d = c; (d as *const u8).add(16);',
        'fn f(p: *const TypedArrayHeader) { let bytes = (p as *const u8); bytes.add(16); }',
        'let p = value.as_pointer::<crate::buffer::BufferHeader>(); let bytes = p.cast::<u8>(); bytes.add(16);',
        'let p = roots.get_raw_mut_ptr::<TypedArrayHeader>(); let alias = p.cast_const(); alias.add(8);',
        'let ptr = crate::buffer::buffer_data(buffer);',
        'let data = typed_array_bytes(ta);',
        'let data = js_value_buffer_or_typedarray_data(value, &mut len);',
        'let data = crate::typedarray::data_ptr_mut(ta);',
        'let b = buffer_alloc(8);\nmark_as_uint8array(b as usize);',
        'let b = buffer_alloc(8);\nlet alias = b;\ncrate::buffer::mark_as_array_buffer(alias as usize);',
        'let quote = \'"\'; // an unmatched " in a comment\nlet data = buffer_data(buffer);',
    ]
    emitted = [f'let data = blk.add(I64, &buffer, "{n}");' for n in [8,16,10]] + [
        'let data = blk.gep(I8, &bytes_header, &[(I32, "16")]);',
        'let data = blk.gep(\n I8, &buffer, &[(I32, "10")]);',
    ]
    with tempfile.TemporaryDirectory(prefix='.buffer-gate-', dir=ROOT) as directory:
        root = Path(directory)
        fixture = root / 'crates/perry-ext-fixture/src/lib.rs'
        emitter = root / 'crates/perry-codegen/src/other.rs'
        fixture.parent.mkdir(parents=True); emitter.parent.mkdir(parents=True)
        for i, (path, planted) in enumerate([(fixture, s) for s in fixtures]+[(emitter, s) for s in emitted]):
            path.write_text(planted+'\n')
            child = subprocess.run([sys.executable, __file__, '--root', str(root)], capture_output=True, text=True)
            assert child.returncode == 1 and 'forbidden:' in child.stdout, (planted, child.stdout, child.stderr)
            print(f'buffer-layout sabotage {i+1}: RED')
            path.unlink()
        fixture.write_text('// buffer_data(buffer);\nlet n = array.length;\n'
                           'let ir = "call ptr @js_native_buffer_data_ptr(double %v)";\n'
                           'let text = r#"buffer_data(buffer)"#;\n'
                           'let owner = buffer_alloc(32); let p = store::data(owner as usize); p.add(8);\n'
                           'let window = data_ptr(owner).add(8);\n')
        assert not inventory(root), 'unrelated arrays, comments and strings must stay outside the gate'
        allowed = root / 'crates/perry-runtime/src/buffer/store/fixture.rs'
        allowed.parent.mkdir(parents=True); allowed.write_text('\n'.join(fixtures))
        abi = root / 'crates/perry-abi/src/lib.rs'
        abi.parent.mkdir(parents=True); abi.write_text('\n'.join(fixtures))
        assert not inventory(root), 'only store and ABI implementations are exempt'


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--self-test', action='store_true')
    ap.add_argument('--root', type=Path, default=ROOT)
    args = ap.parse_args()
    if args.self_test:
        self_test(); return
    current = inventory(args.root)
    for site in current:
        print(f'forbidden: {site}')
    if current: raise SystemExit(1)
    if args.root == ROOT:
        wrapper = (ROOT / 'crates/perry-ffi/src/buffer.rs').read_text()
        assert 'bytes::from_slice' in wrapper
        assert '.add(' not in wrapper and 'copy_nonoverlapping' not in wrapper
    print('buffer-layout gate: 0 sites')


if __name__ == '__main__':
    main()
