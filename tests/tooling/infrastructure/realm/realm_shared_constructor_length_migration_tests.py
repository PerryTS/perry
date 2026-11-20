import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[4] / 'scripts'))

from realm_shared_constructor_length_migration import bindings
class SharedConstructorLengthsTests(unittest.TestCase):
    def test_only_literal_projection_arms_are_shared(self):
        s='fn builtin_constructor_spec_length(n:&str){match n {"A"|"B"=>realm_one!(length),"C"=>1,_=>None}}'
        self.assertEqual(bindings(s),dict(A='realm_one',B='realm_one'))
