import unittest
from realm_reference_capture_migration import classify
class ReferenceCaptureMigrationTests(unittest.TestCase):
    def test_capture_reference_implies_its_birth_edge(self):
        d={'name':'Buffer["@@species"].<get>','kind':'getter','prototype':'Function.prototype','captures':[{'kind':'reference','value':'%BufferSpeciesConstructor%'}]}
        def original(candidate):
            self.assertEqual(candidate['captures'],[])
            return {'constructible':True,'births':['%BufferSpeciesConstructor%']}
        self.assertEqual(classify(d,original),{'constructible':True,'capture':'[ref("%BufferSpeciesConstructor%")]'})
