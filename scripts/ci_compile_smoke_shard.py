#!/usr/bin/env python3
"""Shard top-level TypeScript plus retained tests/parity/compile-smoke.txt paths."""
import argparse
from pathlib import Path
import sys
import tempfile
import unittest


def sources(root: Path) -> list[Path]:
    """Enumerate retained top-level TS inputs, validating every manifest entry."""
    files = [path for path in root.glob('*.ts') if path.is_file()]
    repository = root.parent.parent.parent
    manifest = repository / 'tests/parity/compile-smoke.txt'
    if manifest.exists():
        seen = set()
        for line in manifest.read_text().splitlines():
            entry = line.strip()
            if not entry or entry.startswith('#'):
                continue
            relative = Path(entry)
            path = repository / relative
            if (relative.is_absolute() or '..' in relative.parts
                    or relative.parts[:2] != ('tests', 'parity')
                    or relative.suffix != '.ts' or not path.is_file()
                    or not path.resolve().is_relative_to((repository / 'tests/parity').resolve())):
                raise ValueError(f'invalid compile-smoke manifest path: {entry}')
            if path.resolve() in seen:
                raise ValueError(f'duplicate compile-smoke manifest path: {entry}')
            seen.add(path.resolve())
            files.append(path)
    if len({path.name for path in files}) != len(files):
        raise ValueError('compile-smoke sources must have unique basenames for CI markers')
    # Basename order retains shard assignment when a legacy source moves.
    files.sort(key=lambda path: (path.name, str(path)))
    return files


def select(root, shard):
    try:
        index, total = map(int, shard.split('/'))
    except ValueError as error:
        raise ValueError('shard must be INDEX/TOTAL') from error
    if not 1 <= index <= total:
        raise ValueError('shard must satisfy 1 <= INDEX <= TOTAL')
    if not root.is_dir():
        raise ValueError('test directory does not exist')
    files = sources(root)
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

    def test_moved_cases_keep_their_compile_inventory_and_shard(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            root = repo / 'tests/fixtures/programs'
            root.mkdir(parents=True)
            for name in ['a.ts', 'b.ts', 'c.ts', 'd.ts']:
                (root / name).touch()
            before = [[p.name for p in select(root, f'{i}/2')] for i in (1, 2)]
            moved = repo / 'tests/parity/language/module/fixtures/b.ts'
            moved.parent.mkdir(parents=True)
            (root / 'b.ts').rename(moved)
            manifest = repo / 'tests/parity/compile-smoke.txt'
            manifest.write_text('# Retained standalone compile coverage.\n'
                                'tests/parity/language/module/fixtures/b.ts\n')
            after = [select(root, f'{i}/2') for i in (1, 2)]
            self.assertEqual([[p.name for p in part] for part in after], before)
            self.assertEqual(sum(part.count(moved) for part in after), 1)

    def test_stale_duplicate_and_escaping_manifest_fail_before_selection(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            root = repo / 'tests/fixtures/programs'
            root.mkdir(parents=True)
            (root / 'a.ts').touch()
            manifest = repo / 'tests/parity/compile-smoke.txt'
            manifest.parent.mkdir(parents=True)
            case = repo / 'tests/parity/language/array/b.ts'
            case.parent.mkdir(parents=True)
            case.touch()
            (case.parent / 'a.ts').touch()
            for content in ['tests/parity/missing.ts\n',
                            'tests/parity/language/array/b.ts\n' * 2,
                            '../outside.ts\n', '/outside.ts\n',
                            'tests/fixtures/programs/a.ts\n',
                            'tests/parity/language/array/a.ts\n']:
                with self.subTest(content=content), self.assertRaises(ValueError):
                    manifest.write_text(content)
                    select(root, '1/1')

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
    parser.add_argument('--root', type=Path, default=Path('tests/fixtures/programs'))
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
