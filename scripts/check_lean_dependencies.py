#!/usr/bin/env python3
"""Assert pull-on-use dependency boundaries in Cargo's selected graphs.

Normal/build edges only: test oracles do not ship. Run packages separately so
workspace feature unification cannot conceal a missing or unwanted dependency.
"""
import argparse
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def graph(package, features=None, defaults=False, offline=False):
    cmd = ['cargo', 'tree', '--locked', '-p', package, '--edges', 'normal,build',
           '--prefix', 'none', '--format', '{p}']
    if offline:
        cmd.append('--offline')
    if not defaults:
        cmd.append('--no-default-features')
    if features:
        cmd += ['--features', features]
    result = subprocess.run(cmd, cwd=ROOT, check=True, capture_output=True, text=True)
    names = {line.split()[0] for line in result.stdout.splitlines() if line.strip()}
    assert package in names, f'{package}: empty/unrecognized graph'
    return names


def verify(names, *, absent=(), present=()):
    unwanted = set(absent) & names
    missing = set(present) - names
    assert not unwanted, f'unwanted dependencies: {sorted(unwanted)}'
    assert not missing, f'missing required dependencies: {sorted(missing)}'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--offline', action='store_true')
    args = parser.parse_args()
    cases = [
        ('perry', 'dev-cli', False, ['p256', 'jsonwebtoken', 'chrono', 'env_logger', 'indicatif', 'dotenvy', 'regex', 'regex-automata', 'bzip2', 'ppmd-rust', 'deflate64'], []),
        ('perry', None, True, ['jsonwebtoken', 'chrono', 'env_logger', 'indicatif', 'dotenvy', 'regex', 'regex-automata', 'bzip2', 'ppmd-rust', 'deflate64'], ['p256']),
        ('perry', 'extended-zip', True, [], ['bzip2', 'ppmd-rust', 'deflate64']),
        ('perry-stdlib', None, False, ['uuid', 'chrono'], []),
        ('perry-stdlib', 'crypto', False, ['uuid', 'chrono'], ['getrandom', 'perry-hex', 'perry-base64']),
        ('perry-runtime', None, False, ['resolv-conf', 'lazy_static'], []),
    ]
    for package, features, defaults, absent, present in cases:
        names = graph(package, features, defaults, args.offline)
        verify(names, absent=absent, present=present)
        print(f'OK {package} features={features or "-"} defaults={defaults}: {len(names)} package names')
    for package in ['perry-hex', 'perry-base64']:
        assert graph(package, offline=args.offline) == {package}, f'{package}: codec must stay dependency-free'
    # A planted violation must be rejected (the checker cannot silently pass).
    try:
        verify({'perry-stdlib', 'getrandom'}, absent=['getrandom'])
    except AssertionError:
        pass
    else:
        raise AssertionError('dependency exclusion check failed its sabotage probe')


if __name__ == '__main__':
    main()
