import unittest
import realm_site_migration as s
from realm_iterator_prototype_migration import FAMILIES, render, sites


class IteratorPrototypeMigrationTests(unittest.TestCase):
    def test_four_families_share_one_allocator_site(self):
        self.assertEqual(len(sites()), 1)
        self.assertEqual(sites()[0][4], FAMILIES)
        self.assertEqual(len(FAMILIES), 4)

    def test_anchor_selects_the_allocator_after_the_function_head(self):
        source = 'let a = f(0, 0);\nfn g() { let b = f(0, 0); }'
        self.assertEqual(s.locate(source, 'f(0, 0)', 'fn g(', 0), source.rindex('f(0, 0)'))
        self.assertEqual(s.locate(source, 'f(0, 0)'), source.index('f(0, 0)'))

    def test_render_declares_never_move(self):
        d = dict(name='%ArrayIteratorPrototype%', kind='prototype', prototype='%IteratorPrototype%',
                 properties=[], own_key_order=[], own_symbol_order=[], births=['%IteratorPrototype%'])
        self.assertTrue(render(d).endswith(', never_move = true'))
