import importlib.util
import tempfile
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "scripts" / "test262_subset.py"
SPEC = importlib.util.spec_from_file_location("test262_subset", SCRIPT_PATH)
SUBSET = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUBSET)

HEADER = "/*---\ndescription: x\n{extra}---*/\n"


def case(extra="", body=""):
    return HEADER.format(extra=extra) + body


class FullCorpusTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        harness = self.root / "harness"
        harness.mkdir()
        (harness / "sta.js").write_text("function Test262Error() {}")
        (harness / "assert.js").write_text("var assert = {};")
        (harness / "doneprintHandle.js").write_text("function $DONE() {}")
        (harness / "detachArrayBuffer.js").write_text("function $DETACHBUFFER(b) { $262.detachArrayBuffer(b); }")
        self.harness = harness
        (self.root / "test").mkdir()

    def tearDown(self):
        self.tmp.cleanup()

    def discovered(self, files):
        for name, src in files.items():
            path = self.root / "test" / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(src)
        return {rel: meta for rel, _, meta in SUBSET.discover(
            self.root, ["."], set(), True, set(), full_corpus=True)}

    def test_full_corpus_runs_modules_and_shimmed_host_cases(self):
        metas = self.discovered({
            "module.js": case("flags: [module]\n"),
            "detach.js": case("includes: [detachArrayBuffer.js]\n"),
            "gc.js": case(body="$262.gc();"),
            "agent.js": case(body="$262.agent.start('');"),
            "realm.js": case(body="$262.createRealm();"),
            "canblock.js": case("flags: [CanBlockIsTrue]\n"),
        })
        self.assertEqual(metas["module.js"].excluded, "")
        self.assertTrue(metas["detach.js"].host_shim)
        self.assertTrue(metas["gc.js"].host_shim)
        self.assertEqual(metas["agent.js"].excluded, "needs a $262 host API")
        self.assertEqual(metas["realm.js"].excluded, "needs a $262 host API")
        self.assertEqual(metas["canblock.js"].excluded, "flag: CanBlockIsTrue")

    def test_module_publishes_harness_bindings_on_global(self):
        meta = SUBSET.Meta(flags={"module", "async"})
        program = SUBSET.assemble("test();", meta, self.harness, "function print() {}")
        self.assertIn("globalThis.$DONE = $DONE;", program)
        self.assertIn("globalThis.assert = assert;", program)
        self.assertLess(program.index("globalThis.$DONE"), program.index("test();"))

    def test_script_case_gets_no_global_publication_or_shim(self):
        program = SUBSET.assemble("test();", SUBSET.Meta(), self.harness, "")
        self.assertNotIn("globalThis.", program)
        self.assertNotIn("var $262", program)

    def test_host_shim_precedes_includes(self):
        meta = SUBSET.Meta(includes=["detachArrayBuffer.js"], host_shim=True)
        program = SUBSET.assemble("test();", meta, self.harness, "")
        self.assertLess(program.index("var $262"), program.index("function $DETACHBUFFER"))


if __name__ == "__main__":
    unittest.main()
