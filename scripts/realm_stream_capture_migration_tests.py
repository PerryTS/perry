import unittest
from realm_stream_capture_migration import bindings
class StreamCaptureMigrationTests(unittest.TestCase):
    def test_owner_operation_and_length_come_from_the_same_birth_row(self):
        source='const STREAM_STATIC_READABLE_FROM: f64 = 1.0;\nmatch name {"Readable" => {attach_stream_static(&ctor,"from",STREAM_STATIC_READABLE_FROM,2);}}'
        row=bindings(source)['node:stream.Readable.from']
        self.assertEqual(row[:2],('STREAM_STATIC_READABLE_FROM','3ff0000000000000'))
        self.assertEqual(row[2][0],'2')
