#!/usr/bin/env python3
"""BSD archive alignment does not weaken the archive rewrite payload guard."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "strip_archive_members", Path(__file__).parents[3] / 'scripts/strip-archive-members.py')
strip = importlib.util.module_from_spec(spec)
spec.loader.exec_module(strip)


class MemberPayloadTests(unittest.TestCase):
    def test_bsd_rewrite_may_add_only_the_alignment_fill(self):
        # LLVM 22 adds seven newlines to the 697-byte yoke member in the
        # Darwin HTTP archive while rebuilding the BSD symbol table.
        payload = b"metadata" + b"x" * 688 + b"\n"
        self.assertEqual(len(payload), 697)
        member = ("metadata.o", b"0".ljust(12), b"0".ljust(6),
                  b"0".ljust(6), b"100644".ljust(8), payload)
        before_layout, before = strip.parse(strip.build_bsd([member]))
        padded = (*member[:5], payload + b"\n" * 7)
        after_layout, after = strip.parse(strip.build_bsd([padded]))
        self.assertEqual(before_layout, after_layout)
        self.assertTrue(strip.same_member_payload("bsd", before[0][5], after[0][5]))
        self.assertTrue(strip.same_member_payload("bsd", payload, payload))

    def test_real_content_changes_are_rejected_even_with_valid_fill(self):
        payload = b"real content\n"
        for position in range(len(payload)):
            changed = bytearray(payload)
            changed[position] ^= 1
            with self.subTest(position=position):
                self.assertFalse(strip.same_member_payload(
                    "bsd", payload, bytes(changed) + b"\n" * 3))

    def test_other_suffixes_and_gnu_padding_are_rejected(self):
        payload = b"real content\n"
        for suffix in [b"\n", b"\n" * 2, b"\n" * 4, b"\n" * 11, b"   "]:
            with self.subTest(suffix=suffix):
                self.assertFalse(strip.same_member_payload("bsd", payload, payload + suffix))
        self.assertFalse(strip.same_member_payload("gnu", payload, payload + b"\n" * 3))
        self.assertFalse(strip.same_member_payload("bsd", b"12345678", b"12345678\n"))
        self.assertFalse(strip.same_member_payload("bsd", payload, payload[:-1]))


if __name__ == "__main__":
    unittest.main()
