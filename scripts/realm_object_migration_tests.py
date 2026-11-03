import unittest
from realm_object_migration import births
class ObjectMigrationTests(unittest.TestCase):
    def test_complete_scalar_birth_is_selected_but_dynamic_prefix_is_not(self):
        source='fn make(){\nlet obj=js_object_alloc(0,0);\nnative_set_field(obj,"x",native_string_value("value"));\nnative_set_field(obj,"y",native_bool_value(false));\nreturn obj;\n}'
        found=births(source)
        self.assertEqual(len(found),1)
        self.assertEqual(found[0]["line"],2)
        self.assertEqual(found[0]["facts"]["own_key_order"],["x","y"])
        self.assertEqual(births(source.replace("native_bool_value(false)","dynamic_value()")),[])
if __name__=="__main__":unittest.main()
