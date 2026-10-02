#!/usr/bin/env python3
"""Select a stable slice of the complete top-level TypeScript smoke inventory."""
import argparse
from pathlib import Path
import sys
import tempfile
import unittest


def select(root, shard):
    try:
        index, total = map(int, shard.split('/'))
    except ValueError as error:
        raise ValueError('shard must be INDEX/TOTAL') from error
    if not 1 <= index <= total:
        raise ValueError('shard must satisfy 1 <= INDEX <= TOTAL')
    if not root.is_dir():
        raise ValueError('test directory does not exist')
    files = sorted(path for path in root.glob('*.ts') if path.is_file())
    if not files or len(files) < total:
        raise ValueError('every shard must contain at least one test')
    return files[index - 1::total]


class CoverageTests(unittest.TestCase):
    def test_all_files_run_once_with_balanced_nonempty_shards(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ['a.ts', 'b space.ts', 'c\nnewline.ts', 'd.ts', 'e.ts']:
                (root / name).touch()
            (root / 'ignored.js').touch()
            (root / 'directory.ts').mkdir()
            (root / 'nested').mkdir()
            (root / 'nested/fixture.ts').touch()
            expected = {p for p in root.glob('*.ts') if p.is_file()}
            slices = [select(root, f'{i}/4') for i in range(1, 5)]
            assigned = [p for part in slices for p in part]
            self.assertEqual(set(assigned), expected)
            self.assertEqual(len(assigned), len(expected))
            self.assertLessEqual(max(map(len, slices)) - min(map(len, slices)), 1)
            self.assertTrue(all(slices))
            self.assertEqual(select(root, '1/4'), slices[0])

    def test_invalid_or_empty_scope_cannot_succeed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'a.ts').touch()
            for shard in ['0/4', '5/4', '1/0', '-1/4', '1/2', 'bad', '1/2/3']:
                with self.subTest(shard=shard), self.assertRaises(ValueError):
                    select(root, shard)
            with self.assertRaises(ValueError):
                select(root / 'missing', '1/1')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path('test-files'))
    parser.add_argument('--shard')
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    if args.self_test:
        result = unittest.TextTestRunner().run(unittest.defaultTestLoader.loadTestsFromTestCase(CoverageTests))
        return 0 if result.wasSuccessful() else 1
    if not args.shard:
        parser.error('--shard is required')
    try:
        paths = select(args.root, args.shard)
    except ValueError as error:
        parser.error(str(error))
    # Compute and validate the entire selection before emitting any scope.
    sys.stdout.buffer.write(b'\0'.join(str(path).encode() for path in paths) + b'\0')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
