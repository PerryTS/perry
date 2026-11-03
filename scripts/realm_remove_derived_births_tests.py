import unittest
from realm_remove_derived_births import clean
class DerivedBirthCleanupTests(unittest.TestCase):
    def test_keeps_only_extra_semantic_edges(self):
        self.assertEqual(clean('A, kind = constructor, births = [A.prototype, Function.prototype, Promise]'),'A, kind = constructor, births = [Promise]')
        self.assertEqual(clean('A, kind = constructor, instance = B, births = [B]'),'A, kind = constructor, instance = B')
        self.assertEqual(clean('A.x, births = [A.prototype]'),'A.x, births = [A.prototype]')
