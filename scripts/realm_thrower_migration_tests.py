import unittest
from realm_thrower_migration import ATTR, NAME_LITERAL, OLD, rewrite


class ThrowerMigrationTests(unittest.TestCase):
    def test_attribute_sits_on_the_fn_info_invocation_line(self):
        out = rewrite('x ' + OLD + ' y ' + NAME_LITERAL)
        self.assertIn('fn_info!(' + ATTR + ' arguments_throw_type_error', out)
        self.assertIn('realm_thrower_public!(name)', out)
        self.assertNotIn(NAME_LITERAL, out)

    def test_rewrite_refuses_a_missing_or_duplicate_site(self):
        with self.assertRaises(AssertionError):
            rewrite('nothing')
        with self.assertRaises(AssertionError):
            rewrite(OLD + OLD + NAME_LITERAL)
