#!/usr/bin/env python3
"""Version mismatches must stop catalog writers before they touch output."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

DOCS = Path(__file__).resolve().parents[1]


class ToolchainTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for name in ('i18n.sh', 'i18n-toolchain.env'):
            shutil.copyfile(DOCS / name, self.root / name)
        (self.root / 'po').mkdir()
        (self.root / 'po/messages.pot').write_text('original template\n')
        (self.root / 'po/de.po').write_text('original translation\n')
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ['PATH'])
        self.pins = dict(line.split('=', 1) for line in
                         (DOCS / 'i18n-toolchain.env').read_text().splitlines()
                         if line and not line.startswith('#'))

    def tool(self, name, version):
        executable = self.bin / name
        executable.write_text(f'''#!/bin/sh
if [ "$1" = --version ]; then
    echo '{name} {version}'
else
    echo called >> '{self.root}/writer-called'
fi
''')
        executable.chmod(0o755)

    def run_helper(self, *args):
        return subprocess.run(['bash', str(self.root / 'i18n.sh'), *args],
                              env=self.env, text=True, capture_output=True)

    def assert_rejected(self, result, tool):
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(f'{tool} must be', result.stderr)
        self.assertFalse((self.root / 'writer-called').exists())
        self.assertEqual((self.root / 'po/de.po').read_text(), 'original translation\n')
        self.assertEqual((self.root / 'po/messages.pot').read_text(), 'original template\n')

    def test_wrong_msgmerge_cannot_rewrite_existing_catalog(self):
        self.tool('msgmerge', '999.0')
        self.assert_rejected(self.run_helper('sync'), 'msgmerge')

    def test_wrong_msginit_cannot_create_catalog(self):
        self.tool('msginit', '999.0')
        self.assert_rejected(self.run_helper('add', 'fr'), 'msginit')
        self.assertFalse((self.root / 'po/fr.po').exists())

    def test_wrong_mdbook_cannot_extract(self):
        self.tool('mdbook', 'v999.0')
        self.assert_rejected(self.run_helper('extract'), 'mdbook')

    def test_matching_msgmerge_runs_writer(self):
        self.tool('msgmerge', self.pins['GETTEXT_VERSION'])
        result = self.run_helper('sync')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.root / 'writer-called').read_text(), 'called\n')


if __name__ == '__main__':
    unittest.main()
