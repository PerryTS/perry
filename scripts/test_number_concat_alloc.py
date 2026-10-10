#!/usr/bin/env python3
"""Allocation regression gate for an already-compiled perf_number_concat_alloc.ts."""
import argparse
import re
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('binary', help='compiled perf_number_concat_alloc.ts executable')
args = parser.parse_args()
result = subprocess.run([args.binary], capture_output=True, text=True)
match = re.search(r'^bytes=(\d+),(\d+)$', result.stdout, re.M)
assert match, result.stdout + result.stderr
positive, negative = map(int, match.groups())
assert positive > 0, 'the fixture must allocate a result'
assert negative == positive, (
    f'negative numeric concatenation allocated {negative - positive} extra bytes; '
    f'positive={positive}, negative={negative}'
)
assert result.returncode == 0, result.stderr
print('numeric concatenation: one result allocation for either sign')
