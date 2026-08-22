import importlib.util
import sys
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "node_core_subset.py"
SPEC = importlib.util.spec_from_file_location("node_core_subset", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class NormalizeTests(unittest.TestCase):
    def test_canonicalizes_generated_http_date_headers(self):
        node = (
            '> data "HTTP/1.1 200 OK\\r\\n'
            'Date: Sat, 22 Aug 2026 20:15:36 GMT\\r\\n\\r\\n"'
        )
        perry = (
            '> data "HTTP/1.1 200 OK\\r\\n'
            'Date: Sat, 22 Aug 2026 20:33:38 GMT\\r\\n\\r\\n"'
        )
        self.assertEqual(MODULE.normalize(node), MODULE.normalize(perry))

    def test_preserves_malformed_or_non_http_dates(self):
        value = "Date: tomorrow\ncreated Sat, 22 Aug 2026 20:15:36 GMT"
        self.assertEqual(MODULE.normalize(value), value)


if __name__ == "__main__":
    unittest.main()
