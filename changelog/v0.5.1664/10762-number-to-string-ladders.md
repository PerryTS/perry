**Plain numbers no longer take the slow arms of the string-coercion ladders.**

`js_string_coerce` and `js_jsvalue_to_string_method` reached their plain-number arm last, through a seven-way jump table, though `is_number()` is a single range test and the exact complement of the arms it skips. `js_number_to_string`'s admission check also forced a 14-instruction saturating `f64`→`u64` cast on a value already proven to be within the 256-entry small-integer cache.

`n.toString()` **−19.3%** (536.3 → 433.0), `String(n)` **−14.2%** (190.0 → 163.0), template literal −4.7%, large integers −3.2%, floats −0.9%, `"" + n` unchanged. No row regresses.

`n.toString()` was costing `String(n)` **plus exactly 103 instructions** at every value range — a four-frame dispatch detour through a thread-local one-shot — which is what this removes.
