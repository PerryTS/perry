import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[4] / 'scripts'))

from realm_reference_capture_migration import classify
class ReferenceCaptureMigrationTests(unittest.TestCase):
    def test_capture_reference_implies_its_birth_edge(self):
        d={'name':'Buffer["@@species"].<get>','kind':'getter','prototype':'Function.prototype','captures':[{'kind':'reference','value':'%BufferSpeciesConstructor%'}]}
        def original(candidate):
            self.assertEqual(candidate['captures'],[])
            return {'constructible':True,'births':['%BufferSpeciesConstructor%']}
        self.assertEqual(classify(d,original),{'constructible':True,'capture':'[ref("%BufferSpeciesConstructor%")]'})
