"""Exercise the shipping gate with real ar archives, including failed subjects."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ReleaseTokioTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = Path(self.tmp.name)
        self.dist = self.path / "dist"
        self.dist.mkdir()
        self.obj = self.path / "fixture.o"
        subprocess.run(
            ["cc", "-x", "c", "-c", "-o", str(self.obj), "-"],
            input="int release_fixture = 1;", text=True, check=True,
            capture_output=True,
        )

    def archive(self, name, *members):
        paths = []
        for name_part in members:
            path = self.path / name_part
            shutil.copyfile(self.obj, path)
            paths.append(str(path))
        subprocess.run(["ar", "rcs", str(self.dist / name), *paths],
                       check=True, capture_output=True)

    def gate(self, expected):
        run = subprocess.run(
            ["bash", "-e", str(ROOT / "scripts/check_release_tokio.sh"), str(self.dist)],
            text=True, capture_output=True,
        )
        self.assertEqual(run.returncode == 0, expected, run.stdout + run.stderr)
        return run.stdout + run.stderr

    def stdlib(self):
        self.archive("libperry_stdlib.a", "tokio-abcdef01.tokio.0.rcgu.o")

    def test_cpu_only_wrapper_before_coherent_wrapper(self):
        self.stdlib()
        self.archive("libperry_ext_a.a", "cpu-1234.0.rcgu.o")
        self.archive("libperry_ext_b.a", "leaf-9876.tokio-abcdef01.tokio.0.rcgu.o")
        self.assertIn("1 Tokio-using ext archive(s)", self.gate(True))

    def test_different_tokio_is_rejected(self):
        self.stdlib()
        self.archive("libperry_ext_b.a", "tokio-abcdef02.tokio.0.rcgu.o")
        self.assertIn("disagree", self.gate(False))

    def test_extra_tokio_cannot_hide_behind_matching_first_id(self):
        self.stdlib()
        self.archive("libperry_ext_b.a", "tokio-abcdef01.tokio.0.rcgu.o",
                     "tokio-abcdef02.tokio.0.rcgu.o")
        self.assertIn("disagree", self.gate(False))

    def test_no_compared_archive_fails(self):
        self.stdlib()
        self.archive("libperry_ext_a.a", "cpu-1234.0.rcgu.o")
        self.assertIn("no Tokio-using ext archive", self.gate(False))

    def test_stdlib_without_tokio_fails(self):
        self.archive("libperry_stdlib.a", "cpu-1234.0.rcgu.o")
        self.assertIn("exactly one", self.gate(False))

    def test_stdlib_with_two_tokios_fails(self):
        self.archive("libperry_stdlib.a", "tokio-abcdef01.tokio.0.rcgu.o",
                     "tokio-abcdef02.tokio.0.rcgu.o")
        self.assertIn("exactly one", self.gate(False))

    def test_corrupt_wrapper_is_not_cpu_only(self):
        self.stdlib()
        (self.dist / "libperry_ext_a.a").write_text("not an archive")
        self.gate(False)

    def test_missing_stdlib_fails(self):
        self.gate(False)


class ReleaseMatrixWiringTests(unittest.TestCase):
    def test_every_linux_release_builder_uses_the_extension_gate(self):
        for script in ("build_linux_glibc_2_31.sh", "build_linux_musl.sh"):
            source = (ROOT / "scripts" / script).read_text()
            self.assertIn('bash scripts/build_release_ext.sh "$target"', source, script)

    def test_bullseye_security_snapshot_is_shared_by_linux_images(self):
        expected = (
            "https://snapshot.debian.org/archive/debian-security/"
            "20260901T000000Z/ bullseye-security main"
        )
        for dockerfile in (
            "linux-glibc-2.31.Dockerfile",
            "linux-musl-llvm22.Dockerfile",
        ):
            source = (ROOT / "scripts" / dockerfile).read_text()
            self.assertIn(expected, source, dockerfile)
            self.assertNotIn(
                "https://deb.debian.org/debian-security bullseye-security",
                source,
                dockerfile,
            )


if __name__ == "__main__":
    unittest.main()
