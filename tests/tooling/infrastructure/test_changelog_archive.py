"""Public-interface tests for the per-version changelog archive.

A released version's fragments live in `changelog/vX.Y.Z/` (never deleted), the
release notes are reproducible from that folder, and the release preflight
accepts a tree whose queue was already folded. Driven through the shipped
scripts, never through their internals; `CHANGELOG_ROOT` points every run at a
throwaway fixture repository.
"""
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[3]
FOLD = ROOT / "scripts" / "cut_release_notes.sh"
GATE = ROOT / "scripts" / "check_changelog_archive.py"
SOURCE = ROOT / "scripts" / "release_notes_source.sh"

ENTRIES = ("100-alpha.md", "9-beta.md", "unkeyed-slug.md")


def run_script(script, args, root):
    environment = dict(os.environ, CHANGELOG_ROOT=str(root))
    return _guarded(
        lambda: subprocess.run(
            ["bash", str(script), *args],
            cwd=root,
            env=environment,
            capture_output=True,
            text=True,
        )
    )


def run_gate(root, *args):
    return _guarded(
        lambda: subprocess.run(
            ["python3", str(GATE), "--root", str(root), *args],
            capture_output=True,
            text=True,
        )
    )


def checkout_state():
    """Fingerprint of the real checkout, used to prove isolation."""
    pending = ROOT / "changelog.d"
    archives = ROOT / "changelog"
    return (
        len(list(pending.glob("*.md"))),
        tuple(sorted(p.name for p in archives.iterdir())) if archives.is_dir() else (),
    )


def _guarded(call):
    """Fail loudly if a run ignored CHANGELOG_ROOT and touched the checkout.

    A run that reads the wrong root mutates real data instead of a fixture, so
    the isolation is asserted on every invocation rather than trusted.
    """
    before = checkout_state()
    result = call()
    after = checkout_state()
    if after != before:
        raise AssertionError(
            "the script ignored CHANGELOG_ROOT and mutated the real checkout: "
            f"{before} -> {after}"
        )
    return result


def git(root, *args):
    return subprocess.run(
        ["git", "-C", str(root), *args], check=True, capture_output=True, text=True
    )


class ArchiveTest(unittest.TestCase):
    def fixture(self, entries=ENTRIES):
        root = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, root)
        (root / "changelog.d").mkdir()
        (root / "changelog.d" / "README.md").write_text("# fragments\n")
        for name in entries:
            (root / "changelog.d" / name).write_text(f"entry {name}\n")
        git(root, "init", "-q", "-b", "main")
        git(root, "add", "-A")
        git(
            root,
            "-c",
            "user.name=changelog-archive-test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "fixture",
        )
        return root

    def fold(self, root, version="v1.0.0"):
        done = run_script(FOLD, ["--fold", version, "--no-commit"], root)
        self.assertEqual(done.returncode, 0, done.stderr)
        return root / "changelog" / version

    def test_fold_archives_entries_and_never_deletes(self):
        root = self.fixture()
        archive = self.fold(root)
        self.assertEqual(
            sorted(p.name for p in archive.iterdir()),
            sorted([*ENTRIES, "manifest.json"]),
        )
        self.assertEqual(json.loads((archive / "manifest.json").read_text())["entries"], 3)
        self.assertEqual([p.name for p in (root / "changelog.d").iterdir()], ["README.md"])

    def test_notes_reproduce_from_the_archive(self):
        root = self.fixture()
        before = run_script(FOLD, ["--notes-only"], root).stdout
        self.assertIn("entry 100-alpha.md", before)
        self.fold(root)
        (root / "changelog.d" / "999-late.md").write_text("entry late\n")
        after = run_script(FOLD, ["--notes-only", "--version", "v1.0.0"], root).stdout
        self.assertEqual(after, before)
        self.assertNotIn("entry late", after)

    def test_notes_source_accepts_a_folded_release(self):
        root = self.fixture()
        archive = self.fold(root)
        resolved = run_script(SOURCE, ["v1.0.0"], root)
        self.assertEqual(resolved.returncode, 0, resolved.stderr)
        self.assertEqual(resolved.stdout.strip(), "changelog/v1.0.0")
        self.assertEqual(run_gate(root).returncode, 0)
        # A fragment merged after the fold belongs to the next release, so the
        # folded version keeps resolving to its own folder.
        (root / "changelog.d" / "999-late.md").write_text("entry late\n")
        still = run_script(SOURCE, ["v1.0.0"], root)
        self.assertEqual(still.stdout.strip(), archive.relative_to(root).as_posix())
        self.assertEqual(run_script(SOURCE, ["v9.9.9"], root).stdout.strip(), "changelog.d")

    def test_notes_source_refuses_an_empty_tree(self):
        root = self.fixture(entries=())
        self.assertNotEqual(run_script(SOURCE, ["v1.0.0"], root).returncode, 0)

    def test_gate_rejects_a_malformed_archive(self):
        root = self.fixture()
        archive = self.fold(root)
        (archive / "manifest.json").unlink()
        broken = run_gate(root)
        self.assertEqual(broken.returncode, 1)
        self.assertIn("manifest.json is missing", broken.stderr)

    def test_gate_rejects_a_fragment_that_is_both_archived_and_pending(self):
        root = self.fixture()
        self.fold(root)
        (root / "changelog.d" / "100-alpha.md").write_text("entry again\n")
        duplicated = run_gate(root)
        self.assertEqual(duplicated.returncode, 1)
        self.assertIn("also pending", duplicated.stderr)

    def test_gate_is_not_vacuous_without_an_archive_root(self):
        root = self.fixture()
        absent = run_gate(root)
        self.assertEqual(absent.returncode, 1)
        self.assertIn("changelog is missing", absent.stderr)


if __name__ == "__main__":
    unittest.main()
