"""Exercise the gate with deliberately broken archives and native executables."""

import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "cc_parity_gate", Path(__file__).resolve().parents[1] / "scripts/cc_parity_gate.py"
)
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)


def identity(data):
    return {"bytes": len(data), "sha256": gate.digest(data)}


class GateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)
        (self.work / "logs").mkdir()
        self.manifest = {"bundle": identity(b"bundle"), "goldens": {}}
        for case, data in (("help", b"help\n"), ("version", b"version\n")):
            (self.work / f"{case}.stdout").write_bytes(data)
            self.manifest["goldens"][case] = identity(data)

    def archive(self, source=b"bundle", symlink=False):
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
            member = tarfile.TarInfo("package/cli.js")
            member.size = len(source)
            if symlink:
                member.type = tarfile.SYMTYPE
                member.linkname = "../../outside"
            archive.addfile(member, io.BytesIO(source))
        data = buffer.getvalue()
        path = self.work / "package.tgz"
        path.write_bytes(data)
        self.manifest["archive"] = identity(data)
        return path

    def test_prepare_checks_both_hashes(self):
        archive = self.archive()
        gate.prepare(self.work, self.manifest, archive)
        self.assertEqual((self.work / "cli.js").read_bytes(), b"bundle")
        archive.write_bytes(archive.read_bytes() + b"changed")
        with self.assertRaisesRegex(ValueError, "npm archive"):
            gate.prepare(self.work, self.manifest, archive)
        archive = self.archive(b"different bundle")
        with self.assertRaisesRegex(ValueError, "cli.js"):
            gate.prepare(self.work, self.manifest, archive)

    def test_prepare_rejects_symlink(self):
        with self.assertRaisesRegex(ValueError, "regular file"):
            gate.prepare(self.work, self.manifest, self.archive(symlink=True))

    def test_rejects_script_in_native_arm(self):
        binary = self.work / "claude-native"
        binary.write_text("#!/bin/sh\necho help\n")
        binary.chmod(0o755)
        with self.assertRaisesRegex(ValueError, "Mach-O"):
            gate.require_native(binary)

    def test_failed_compile_cannot_reuse_a_stale_binary(self):
        (self.work / "cli.js").write_bytes(b"bundle")
        binary = self.work / "claude-native"
        binary.write_bytes(b"old executable")
        compiler = self.work / "failing-perry"
        compiler.write_text("#!/bin/sh\necho deliberate compiler failure >&2\nexit 2\n")
        compiler.chmod(0o755)
        with self.assertRaisesRegex(ValueError, "native compilation failed"):
            gate.compile_bundle(self.work, self.manifest, compiler, timeout=5)
        self.assertFalse(binary.exists())
        report = json.loads((self.work / "logs/compile.json").read_text())
        self.assertEqual(report["exit_code"], 2)

    def test_scratch_environment_is_an_allowlist(self):
        env = gate.scratch_env(self.work)
        self.assertEqual(env["HOME"], str(self.work))
        self.assertEqual(env["TMPDIR"], str(self.work))
        self.assertNotIn("ANTHROPIC_API_KEY", env)
        self.assertFalse(any(key.startswith("PERRY_") for key in env))
        self.assertNotIn("/opt/homebrew/bin", env["PATH"])

    def test_checked_in_golden_integrity(self):
        manifest = json.loads((gate.CORPUS / "manifest.json").read_text())
        self.assertEqual(manifest["goldens"]["help"]["bytes"], 9175)
        for case in gate.CASES:
            gate.verify((gate.CORPUS / f"{case}.stdout").read_bytes(),
                        manifest["goldens"][case], case)


@unittest.skipUnless(sys.platform == "darwin", "native offline gate uses macOS Seatbelt")
class NativeGateTests(unittest.TestCase):
    setUp = GateTests.setUp

    def native(self, behavior=""):
        source = self.work / "fixture.c"
        source.write_text('''#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <errno.h>
int main(int argc, char **argv) {
    if (argc != 2 || getenv("ANTHROPIC_API_KEY") || getenv("PERRY_TEST_KNOB")) return 8;
    if (!getenv("HOME") || !strstr(getenv("HOME"), "cc-parity-")) return 9;
    // Prove Seatbelt is live, even though these fixtures need no connection.
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd >= 0) {
        struct sockaddr_in address = {0};
        address.sin_family = AF_INET;
        address.sin_port = htons(9);
        address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        int result = connect(fd, (struct sockaddr *)&address, sizeof(address));
        int error = errno;
        close(fd);
        if (result != -1 || error != EPERM) return 10;
    } else if (errno != EPERM) return 11;
    ''' + behavior + '''
    puts(strcmp(argv[1], "--help") == 0 ? "help" : "version");
    return 0;
}
''')
        subprocess.run(["/usr/bin/cc", str(source), "-o", str(self.work / "claude-native")],
                       check=True, capture_output=True)

    def check(self, timeout=5):
        gate.check(self.work, self.manifest, corpus=self.work, timeout=timeout)

    def test_native_exact_bytes_pass_with_network_denied(self):
        self.native()
        self.check()
        report = json.loads((self.work / "logs/parity.json").read_text())
        self.assertEqual(set(report["cases"]), {"help", "version"})
        self.assertTrue(all(case["passed"] for case in report["cases"].values()))

    def test_one_byte_difference_fails(self):
        self.native('putchar(\'!\');')
        with self.assertRaisesRegex(ValueError, "parity failed"):
            self.check()

    def test_nonzero_exit_fails_even_with_matching_stdout(self):
        self.native('puts(strcmp(argv[1], "--help") == 0 ? "help" : "version"); return 3;')
        with self.assertRaisesRegex(ValueError, "parity failed"):
            self.check()
        report = json.loads((self.work / "logs/parity.json").read_text())
        self.assertTrue(report["cases"]["help"]["matches_golden"])

    def test_timeout_fails(self):
        self.native("sleep(10);")
        with self.assertRaisesRegex(ValueError, "parity failed"):
            self.check(timeout=0.2)
        report = json.loads((self.work / "logs/parity.json").read_text())
        self.assertTrue(report["cases"]["help"]["timed_out"])

    def test_changed_golden_fails_before_execution(self):
        self.native()
        (self.work / "help.stdout").write_bytes(b"incorrect golden\n")
        with self.assertRaisesRegex(ValueError, "help golden"):
            self.check()
        self.assertFalse((self.work / "logs/help.stdout").exists())


if __name__ == "__main__":
    unittest.main()
