from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[1]


def _function_body(source: str, name: str) -> str:
    match = re.search(rf"function {re.escape(name)}\([^)]*\) \{{", source)
    if not match:
        raise AssertionError(f"missing {name}")
    start = match.end()
    depth = 1
    i = start
    while i < len(source) and depth:
        if source[i] == "{":
            depth += 1
        elif source[i] == "}":
            depth -= 1
        i += 1
    if depth != 0:
        raise AssertionError(f"unterminated {name}")
    return source[start : i - 1]


class WebKeyboardPollingTests(unittest.TestCase):
    def test_wasm_keyboard_polling_installs_dom_listeners(self) -> None:
        source = (ROOT / "crates/perry-codegen-wasm/src/wasm_runtime.js").read_text()

        is_down = _function_body(source, "perry_ui_is_key_down")
        modifiers = _function_body(source, "perry_ui_current_modifiers")

        self.assertIn("__perryEnsureKbdInstalled();", is_down)
        self.assertIn("__perryEnsureKbdInstalled();", modifiers)

    def test_js_keyboard_polling_installs_dom_listeners(self) -> None:
        source = (ROOT / "crates/perry-codegen-js/src/web_runtime.js").read_text()

        is_down = _function_body(source, "perry_ui_is_key_down")
        modifiers = _function_body(source, "perry_ui_current_modifiers")

        self.assertIn("_perryEnsureKbd();", is_down)
        self.assertIn("_perryEnsureKbd();", modifiers)


if __name__ == "__main__":
    unittest.main()
