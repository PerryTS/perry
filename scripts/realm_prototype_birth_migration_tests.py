import unittest
from realm_prototype_birth_migration import members, render
class PrototypeBirthMigrationTests(unittest.TestCase):
    def test_defaults_aliases_symbols_and_extra_edges(self):
        d=dict(name='A.prototype',kind='prototype',prototype='Object.prototype',births=['A','A.prototype.method','Object.prototype','Promise'],
               own_key_order=['constructor','alias'],own_symbol_order=['@@toStringTag'],properties=[
                   dict(key='constructor',symbol=False,attributes=5,value=dict(kind='reference',value='A')),
                   dict(key='alias',symbol=False,attributes=7,value=dict(kind='reference',value='A.prototype.method')),
                   dict(key='@@toStringTag',symbol=True,attributes=4,value=dict(kind='string',value='A'))])
        text=render(d)
        self.assertIn('constructor; alias = ref(A.prototype.method), attrs = writable | enumerable | configurable',text)
        self.assertIn('[Symbol.toStringTag] = "A"',text)
        self.assertIn('births = [Promise]',text)
        self.assertNotIn('header',text)
        self.assertNotIn('Object.prototype',text)
