import unittest
from realm_color_migration import birth_site,color_rows
class ColorMigrationTests(unittest.TestCase):
    def test_values_stay_in_typed_rows_and_carrier_is_adjacent_to_birth(self):
        self.assertEqual(color_rows('const INSPECT_COLOR_STYLES:&[AnsiStyle]=&[AnsiStyle{name:"red",open:31,close:39}];'),[("red",31,39)])
        source='fn util_inspect_colors(){\n\n let obj=js_object_alloc(0,0);\n}'
        offset,line=birth_site(source)
        self.assertEqual(line,2)
        self.assertEqual(source[offset],"\n")
if __name__=="__main__":unittest.main()
