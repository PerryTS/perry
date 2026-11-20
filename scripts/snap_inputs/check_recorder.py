#!/usr/bin/env python3
"""Assert recorder survival, ordered complete rows, and the actual GC boundary."""
import csv
import subprocess
import sys
from pathlib import Path
root = Path(sys.argv[1])
arm = sys.argv[2] if len(sys.argv) > 2 else 'guard-head'
for fixture in ('effect', 'shapes', 'abort'):
    for watch in (0, 1):
        d = root / 'fx' / fixture / arm
        path = d / f'report-{watch}.inputs.tsv'
        assert path.is_file() and path.read_bytes().endswith(b'\n'), path
        rows = list(csv.DictReader(path.open(), delimiter='\t'))
        assert rows and all(set(r) == set(rows[0]) and None not in r.values() for r in rows), path
        assert [int(r['seq']) for r in rows] == list(range(1, len(rows) + 1)), path
        for row in rows:
            bytes.fromhex(row['value_hex'])
        if fixture == 'abort':
            subprocess.run([sys.executable, str(Path(__file__).with_name('check_abort.py')), str(path)], check=True)
        else:
            assert any(r['op'] == 'boundary' for r in rows), f'{path}: GC boundary never reached'
            assert (d / f'report-{watch}.live.tsv').is_file(), f'{path}: GC completion missing'
            stdout = (d / f'stdout-{watch}').read_text().split()
            assert stdout == (['function'] if fixture == 'effect' else ['1000'] * 8), stdout
        print(f'{fixture} watch={watch}: complete ordered prefix, {len(rows)} events')
