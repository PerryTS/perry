import unittest
from realm_module_provider_migration import provider_item
class ModuleProviderMigrationTests(unittest.TestCase):
    def test_export_attributes_and_docs_stay_owned_by_the_function(self):
        source='fn other() {}\n\n/// Provider docs.\n/// # Safety\n#[no_mangle]\npub unsafe extern "C" fn provider() { if true { work(); } }\nfn after() {}'
        a,b=provider_item(source,'provider')
        self.assertTrue(source[a:b].startswith('/// Provider docs.'))
        self.assertTrue(source[a:b].endswith('work(); } }'))
        self.assertNotIn('after',source[a:b])
