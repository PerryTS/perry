#!/usr/bin/env python3
"""Node runtime modules use per-site memo entries for by-name Get.

The dynamic event/symbol-key lane remains computed access. Literal property
access in a new function is checked along with existing helpers: no line or
function allowlist can hide a raw generic by-name call.
"""
from pathlib import Path
import re
import sys

RAW = re.compile(r'\b(?:js_object_get_field_by_name(?:_f64)?|js_object_get_property_key|get_property_key_resolved|js_get_property|js_native_call_method(?:_str_key)?)\b')

def lex(source):
    """Blank strings and comments, preserving positions and Rust syntax."""
    out = list(source)
    i = 0
    while i < len(source):
        if source.startswith('//', i):
            end = source.find('\n', i)
            end = len(source) if end < 0 else end
        elif source.startswith('/*', i):
            end = source.find('*/', i + 2)
            end = len(source) if end < 0 else end + 2
        elif source[i] == '"':
            end = i + 1
            while end < len(source):
                if source[end] == '\\': end += 2
                elif source[end] == '"': end += 1; break
                else: end += 1
        else:
            i += 1
            continue
        for j in range(i, min(end, len(out))):
            if out[j] != '\n': out[j] = ' '
        i = end
    return ''.join(out)

def production(source):
    out = list(lex(source))
    text = ''.join(out)
    for match in re.finditer(r'#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]', text):
        start = match.start()
        i = match.end()
        # Skip other attributes before the test-only item.
        while i < len(text):
            if text[i].isspace(): i += 1
            elif text.startswith('#[', i):
                i = text.find(']', i) + 1
            else: break
        op = re.search(r'[;{]', text[i:])
        if op is None: continue
        end = i + op.start() + 1
        if text[end - 1] == '{':
            depth = 1
            while end < len(text) and depth:
                if text[end] == '{': depth += 1
                elif text[end] == '}': depth -= 1
                end += 1
        for j in range(start, end):
            if out[j] != '\n': out[j] = ' '
    return ''.join(out)

def violations(source):
    return [production(source).count('\n', 0, m.start()) + 1 for m in RAW.finditer(production(source))]

def module_files(root):
    runtime = root / 'crates/perry-runtime/src'
    paths = list(runtime.glob('node_stream*.rs')) + list(runtime.glob('async_hooks*.rs'))
    for prefix in ('node_stream', 'node_stream_constructors', 'async_hooks'):
        paths.extend((runtime / prefix).rglob('*.rs'))
    paths.extend((root / 'crates/perry-ext-http/src/server').glob('response*.rs'))
    return sorted({p for p in paths if not re.search(r'(?:^|_)tests?(?:_|$)', p.stem)})

def self_test():
    assert not re.search(r'(?:^|_)tests?(?:_|$)', 'node_stream_latest')
    good = 'fn read() { runtime_state_key!(b"state").read_value(value); }'
    assert not violations(good)
    for call in ('js_object_get_field_by_name(o, k)', 'crate::object::js_object_get_field_by_name_f64(o, k)', 'js_object_get_property_key(o, k)', 'get_property_key_resolved(o, k)', 'js_get_property(o, name, len)', 'js_native_call_method(o, name, len, args, argc)'):
        assert violations(good + '\nfn extra() { ' + call + '; }') == [2]
        assert not violations('#[cfg(test)]\nfn extra() { ' + call + '; }')
    assert violations('use crate::object::js_object_get_field_by_name_f64 as alias;\nfn extra() { alias(o, k); }') == [1]
    assert not violations('// js_object_get_field_by_name(o, k)\n' + good)
    assert not violations('const DOC: &str = "js_object_get_field_by_name(o, k)";')

if __name__ == '__main__':
    self_test()
    root = Path(__file__).resolve().parent.parent
    bad = [(p, line) for p in module_files(root) for line in violations(p.read_text())]
    for p, line in bad:
        print(f'{p.relative_to(root)}:{line}: raw generic Get bypasses the runtime memo entry')
    if not bad: print(f'runtime named-read invariant: {len(module_files(root))} complete modules checked; negative controls detected')
    sys.exit(bool(bad))
