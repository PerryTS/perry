`scripts/wasi_imports_check.py` reports a truncated WebAssembly module as
invalid instead of crashing with an `IndexError`. A failed link that leaves a
partial module now shows up as a failed imports stage in
`scripts/wasi_run.py`. Follow-up to #13343.
