"""Check bounded Linux THP policy parsing and the mapper's import budget."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


@unittest.skipUnless(sys.platform == 'linux' and shutil.which('cc'), 'Linux native compiler required')
class HotTextPolicyTests(unittest.TestCase):
    def test_only_complete_selected_tokens_enable_backing(self):
        source = Path(__file__).resolve().parents[1] / 'crates/perry-runtime/src/hot_text.c'
        policies = ['always madvise [never]', 'always [madvise] never',
                    '[always] madvise never', '[madvise]', '[never]',
                    'always madvise never', '[Always]', '[alway]', '[madviseX]']
        statements = ['assert(!selected_thp_policy(NULL, 0));']
        for policy in policies:
            for length in range(len(policy) + 1):
                expected = int('[always]' in policy[:length] or '[madvise]' in policy[:length])
                statements.append(f'assert(selected_thp_policy("{policy}", {length}) == {expected});')
        with tempfile.TemporaryDirectory(prefix='perry-hot-policy-') as tmp:
            driver = Path(tmp) / 'policy.c'
            driver.write_text(f'#include "{source}"\n#include <assert.h>\nint main(void) {{\n' +
                              '\n'.join(statements) + '\nreturn 0; }\n')
            exe = Path(tmp) / 'policy'
            subprocess.run(['cc', '-O3', str(driver), '-o', str(exe)], check=True, capture_output=True)
            subprocess.run([str(exe)], check=True, capture_output=True)

    def test_mapper_does_not_add_strstr_or_mremap_imports(self):
        source = Path(__file__).resolve().parents[1] / 'crates/perry-runtime/src/hot_text.c'
        with tempfile.TemporaryDirectory(prefix='perry-hot-policy-') as tmp:
            obj = Path(tmp) / 'mapper.o'
            subprocess.run(['cc', '-O3', '-c', str(source), '-o', str(obj)], check=True, capture_output=True)
            imports = subprocess.check_output(['nm', '--undefined-only', str(obj)], text=True)
            names = {line.split()[-1] for line in imports.splitlines()}
            self.assertNotIn('strstr', names)
            self.assertNotIn('mremap', names)


if __name__ == '__main__':
    unittest.main()
