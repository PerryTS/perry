#!/usr/bin/env python3
"""Regression coverage for #10389 through the runtime symbol scanner itself."""
import tempfile
from pathlib import Path
from gc_root_dominance_check import runtime_symbols, macro_generated_symbols


def main():
    with tempfile.TemporaryDirectory() as tmp:
        source = Path(tmp) / "exports.rs"
        source.write_text('''macro_rules! regex_value {
    ($name:ident, $all:expr) => {
        #[no_mangle] pub extern "C" fn $name() {}
    };
}
regex_value!(js_string_replace_regex_fn, false);
regex_value!(js_string_replace_all_regex_fn, true);
wasm_export_call_shim!(js_wasm_export_call_0);
two_operand_value_entry!(
    /// An attributed multiline invocation.
    js_path_basename_ext_value => super::js_path_basename_ext, f64, KEEP);
pub extern "C-unwind" fn js_literal_unwind() {}
fn body() {
    assert_eq!(js_decoy, 0);
}
''')
        expected = {"js_string_replace_regex_fn", "js_string_replace_all_regex_fn",
                    "js_wasm_export_call_0", "js_path_basename_ext_value"}
        assert macro_generated_symbols((tmp,)) == expected
        assert runtime_symbols((tmp,)) == expected | {"js_literal_unwind"}
        # Removing a macro call removes its symbol; a hard-coded set of names
        # would not pass this control.
        source.write_text(source.read_text().replace(
            "regex_value!(js_string_replace_regex_fn, false);", ""))
        assert "js_string_replace_regex_fn" not in runtime_symbols((tmp,))
    print("GC macro export regression: inline, multiline and unwind symbols visible; indented assertions excluded")


if __name__ == "__main__":
    main()
