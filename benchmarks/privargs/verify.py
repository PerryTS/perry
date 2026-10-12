#!/usr/bin/env python3
"""Compare focused witnesses using the separately built compiler arms."""
import argparse
import json
from pathlib import Path
import subprocess
from measure import environment

def main():
    p = argparse.ArgumentParser()
    p.add_argument('root', type=Path)
    p.add_argument('--arms', nargs='+', default=['main', 'head'])
    a = p.parse_args()
    root = a.root.resolve()
    tests = root / 'head/src/test-files'
    selected = [source for source in [*tests.glob('*.ts'), *tests.glob('*.cts')]
                if any(word in source.name for word in
                       ['arguments', 'private_field', 'private_method', 'private_brand',
                        'private_context', 'private_error', 'privargs', '10515', '10516', '11791',
                        'class_method', 'derived_this', 'lexical_this', 'super_this',
                        'super_arrow', 'generator_method', 'computed_generator',
                        'extracted_method', 'method_receiver'])
                and 'createprivatekey' not in source.name]
    logs = root / 'focused'
    logs.mkdir(exist_ok=True)
    results = {}
    for source in sorted(selected):
        expected = subprocess.run(['node', '--disable-warning=MODULE_TYPELESS_PACKAGE_JSON',
                                   str(source)], capture_output=True, cwd=tests, timeout=60)
        if expected.returncode:
            results[source.name] = {'node_fail': True}
            continue
        row = {}
        for arm in a.arms:
            env = environment(root, arm)
            binary = logs / (arm + '-' + source.stem)
            compile_result = subprocess.run([str(root / arm / 'target/release/perry'),
                                              'compile', str(source), '-o', str(binary)],
                                             capture_output=True, env=env, cwd=tests, timeout=180)
            (logs / (arm + '-' + source.stem + '.compile.log')).write_bytes(compile_result.stdout + compile_result.stderr)
            if compile_result.returncode:
                row[arm] = 'compile_fail'
                continue
            actual = subprocess.run([str(binary)], capture_output=True, env=env, cwd=tests, timeout=60)
            (logs / (arm + '-' + source.stem + '.out')).write_bytes(actual.stdout)
            (logs / (arm + '-' + source.stem + '.err')).write_bytes(actual.stderr)
            (logs / (source.stem + '.node.out')).write_bytes(expected.stdout)
            row[arm] = 'pass' if actual.returncode == 0 and actual.stdout == expected.stdout else 'fail'
        results[source.name] = row
        (root / 'focused.json').write_text(json.dumps(results, indent=2))
        print(source.name, row, flush=True)
    (root / ('focused-' + '-'.join(a.arms) + '.json')).write_text(json.dumps(results, indent=2))
    reference = a.arms[0]
    bad = {key: value for key, value in results.items()
           if value.get(reference) == 'pass'
           and any(value.get(arm) not in [None, 'pass'] for arm in a.arms[1:])}
    print('new failures:', bad, flush=True)
    raise SystemExit(bool(bad))

if __name__ == '__main__':
    main()
