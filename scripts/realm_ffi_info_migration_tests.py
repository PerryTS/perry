import unittest
import realm_source_migration as m
from realm_source_migration_tests import atomics_add
from realm_ffi_info_migration import rewrite
class FfiInfoMigrationTests(unittest.TestCase):
    def test_length_projection_stays_on_the_original_info_line(self):
        d=atomics_add()
        d.update(name='node:net.BlockList.prototype.addAddress',body='perry_ext_net::classes::prototype::add_address',length=1)
        d['function_info'].update(params=2,declared=0,flags=1024)
        d['properties'][0]['value']['value']=m.struct.pack('>d',1).hex()
        d['properties'][1]['value']['value']='addAddress'
        source='''pub(super) unsafe extern "C" fn install_block_list(raw: *mut c_void) {
    proto.method(
        "addAddress",
        perry_ffi::js_function_info!(add_address, 2; with_flags(perry_ffi::FN_BUILTIN)),
        1,
    );
}
'''
        result,placements=rewrite(source,[d])
        self.assertEqual(result.count('\n'),source.count('\n'))
        self.assertEqual(placements[0]['line'],4)
        self.assertIn('spec = { length = 1 }',result.splitlines()[3])
        self.assertIn('realm_add_address_metadata!(length)',result)
        self.assertNotIn('number_bits',result)
        self.assertNotIn('capture_flags',result)
