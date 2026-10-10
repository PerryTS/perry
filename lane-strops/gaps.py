#!/usr/bin/env python3
"""Focused Node parity and GC controls using independently built lane arms."""
import argparse, json, re, shlex, subprocess
from pathlib import Path
from verify import env_for, NODE, save_result

TESTS = ['test_gap_strops_provenance', 'test_string_slice', 'test_string_methods',
         'test_gap_string_methods', 'test_gap_string_index_character',
         'test_gap_string_slice_utf16_suffix', 'test_gap_string_trim_boundaries',
         'test_gap_10725_wtf8_starts_ends_position', 'test_gap_10191_sso_utf16_length',
         'test_gap_10692_normalize_lone_surrogate', 'test_gap_9431_array_from_lone_surrogate',
         'test_parity_string_append_surrogate_repair', 'test_gap_sso_concat_string_index',
         'test_gap_10762_number_to_string_sso', 'test_gap_number_string_2864_2948_2855',
         'test_gap_template_literal_leading_part', 'test_gap_gc_template_coerce_join',
         'test_gap_gc_string_copy_source_rooting', 'test_gap_gc_string_literal_operand_rooting']

p = argparse.ArgumentParser()
p.add_argument('--hostdir', type=Path, required=True)
p.add_argument('--arm', choices=['base', 'head'], default='head')
p.add_argument('--tests', nargs='+', default=TESTS)
a = p.parse_args()
root = a.hostdir
source = root / ('base-src' if a.arm == 'base' else 'src')
out = root / 'gaps' / a.arm
out.mkdir(parents=True, exist_ok=True)
env = env_for(root, a.arm)
compiler = Path(env['CARGO_TARGET_DIR']) / 'release/perry'
rows = {}
for name in a.tests:
    fixture = source / 'test-files' / (name + '.ts')
    binary = out / name
    compiled = subprocess.run([str(compiler), 'compile', str(fixture), '-o', str(binary)],
                              cwd=source, env=env, capture_output=True, timeout=900)
    (out / (name + '.compile.log')).write_bytes(compiled.stdout + compiled.stderr)
    if compiled.returncode:
        rows[name] = {'compile': compiled.returncode, 'parity': False}
    else:
        testenv = dict(env)
        for directive in re.findall(r'^// parity-env: (.*)$', fixture.read_text(), re.M):
            testenv.update(item.split('=', 1) for item in shlex.split(directive))
        node = subprocess.run([*NODE, str(fixture)], cwd=source, env=testenv,
                              capture_output=True, timeout=180)
        actual = subprocess.run([str(binary)], cwd=source, env=testenv,
                                capture_output=True, timeout=180)
        save_result(out / (name + '.node'), node)
        save_result(out / (name + '.perry'), actual)
        rows[name] = {'compile': 0, 'node_exit': node.returncode, 'perry_exit': actual.returncode,
                      'parity': node.returncode == actual.returncode == 0 and node.stdout == actual.stdout
                      and node.stderr == actual.stderr}
    (out / 'results.json').write_text(json.dumps(rows, indent=2))
    print(a.arm, name, rows[name], flush=True)
raise SystemExit(not all(row['parity'] for row in rows.values()))
