import importlib.util
from pathlib import Path
import subprocess
import unittest

path = Path(__file__).resolve().parents[1] / 'scripts/runtime_test_batches.py'
spec = importlib.util.spec_from_file_location('runtime_test_batches', path)
batches = importlib.util.module_from_spec(spec)
spec.loader.exec_module(batches)


class RuntimeTestBatches(unittest.TestCase):
    def test_inventory_is_complete_and_unambiguous(self):
        self.assertEqual(batches.inventory('a: test\nb: test\nbench: benchmark\n'), ['a', 'b'])
        for listing in ['', 'a: test\na: test\n', 'a: test\nnew output format\n']:
            with self.subTest(listing=listing), self.assertRaises(ValueError):
                batches.inventory(listing)

    def runner(self, *, fail_batch=None, incomplete=False):
        calls = []

        def run(command, **kwargs):
            calls.append(command)
            if '--list' in command:
                return subprocess.CompletedProcess(command, 0, 'a: test\nb: test\nc: test\n')
            if '--exact' in command:
                number = sum('--exact' in c for c in calls)
                names = command[command.index('never') + 1:]
                self.assertEqual(command[command.index('--test-threads=1')], '--test-threads=1')
                if fail_batch == number:
                    return subprocess.CompletedProcess(command, 101, 'test failed\n')
                passed = len(names) - 1 if incomplete else len(names)
                return subprocess.CompletedProcess(command, 0,
                    f'test result: ok. {passed} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n')
            return subprocess.CompletedProcess(command, 0, '')

        return calls, run

    def test_each_test_runs_once_and_full_mode_keeps_other_targets(self):
        calls, run = self.runner()
        self.assertEqual(batches.run_batches(['--profile', 'dev'], 2, run=run), 0)
        actual = [name for c in calls if '--exact' in c for name in c[c.index('never') + 1:]]
        self.assertEqual(actual, ['a', 'b', 'c'])
        self.assertEqual(calls[-2][-2:], ['--test', '*'])
        self.assertEqual(calls[-1][-1], '--doc')
        self.assertTrue(all(c[c.index('--profile') + 1] == 'dev' for c in calls))
        for command in (c for c in calls if '--exact' in c):
            self.assertEqual(command[command.index('--format') + 1], 'pretty')
            self.assertIn('--nocapture', command)

    def test_child_process_summaries_do_not_replace_parent_coverage(self):
        _, base_run = self.runner()

        def run(command, **kwargs):
            result = base_run(command, **kwargs)
            if '--exact' in command:
                result.stdout = ('test result: ok. 1 passed; 0 failed; 0 ignored; '
                                 '0 measured; 0 filtered out;\n' + result.stdout)
            return result

        self.assertEqual(batches.run_batches(batch_size=2, lib_only=True, run=run), 0)

    def test_unit_mode_does_not_add_integration_tests(self):
        calls, run = self.runner()
        self.assertEqual(batches.run_batches(batch_size=2, lib_only=True, run=run), 0)
        self.assertEqual(len(calls), 3)

    def test_batch_failure_cannot_be_hidden_by_a_later_success(self):
        calls, run = self.runner(fail_batch=1)
        self.assertEqual(batches.run_batches(batch_size=2, run=run), 101)
        self.assertEqual(len(calls), 2)

    def test_zero_exit_without_exact_coverage_is_rejected(self):
        _, run = self.runner(incomplete=True)
        with self.assertRaisesRegex(ValueError, 'every requested test'):
            batches.run_batches(batch_size=2, run=run)


if __name__ == '__main__':
    unittest.main()
