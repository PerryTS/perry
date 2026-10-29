import unittest
from realm_constructor_shape_migration import shape_facts
class ConstructorShapeMigrationTests(unittest.TestCase):
    def test_inherited_constructor_omits_derived_edges(self):
        d=dict(name='Child',kind='constructor',prototype='Parent',properties=[dict(key='prototype',attributes=1,value=dict(kind='reference',value='Child.prototype'))])
        f=shape_facts(d,lambda x:dict(births=['Parent','Child.prototype','Extra']))
        self.assertEqual(f,dict(extends='Parent',attrs='{prototype = writable}',births=['Extra']))
        self.assertEqual(d['properties'][0]['attributes'],1)
