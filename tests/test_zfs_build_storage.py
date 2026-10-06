"""Agent builds reject storage that cannot provide the configured ZFS policy."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("storage", Path(__file__).resolve().parents[1] / "scripts/check_zfs_build_storage.py")
storage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(storage)


class StorageTests(unittest.TestCase):
    def test_compressed_bounded_dataset_and_rejections(self):
        with tempfile.TemporaryDirectory() as directory:
            props = {"compression": "lz4", "dedup": "off", "quota": "100G"}
            filesystem = "zfs"
            source = "tank/builds"
            def command(*args):
                if args[0] == "findmnt":
                    return filesystem if args[3] == "FSTYPE" else source
                return props[args[-2]]
            self.assertEqual(storage.validate(directory, directory, command), source)
            for prop, bad in (("compression", "off"), ("dedup", "on"), ("quota", "none")):
                previous = props[prop]
                props[prop] = bad
                with self.assertRaises(ValueError):
                    storage.validate(directory, directory, command)
                props[prop] = previous
            filesystem = "ext4"
            with self.assertRaises(ValueError):
                storage.validate(directory, directory, command)
            with self.assertRaises(ValueError):
                storage.validate(None, directory, command)
            filesystem = "zfs"
            calls = 0
            def split(*args):
                nonlocal calls
                if args[0] == "findmnt" and args[3] == "SOURCE":
                    calls += 1
                    return "tank/cache" if calls == 1 else "tank/targets"
                return command(*args)
            with self.assertRaises(ValueError):
                storage.validate(directory, directory, split)
