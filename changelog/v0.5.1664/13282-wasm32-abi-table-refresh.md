Refresh the wasm32 runtime ABI table (`runtime_abi.tsv`) and teach
`scripts/runtime_abi_check.py` to read the prototype-entry macro family
(`array_entries!` / `string_entries!`), whose exported `js_array_m_*`,
`js_array_b_*`, `js_string_m_*` and `js_string_b_*` entries were missing from
the table. The table had also gone stale: `js_register_path_module_partial`
was recorded with two parameters while the runtime takes three, so wasm32
calls to it were rewritten to the wrong arity. `--check-wasm-abi` is green
again, and the script's self-test covers the new macro form.
