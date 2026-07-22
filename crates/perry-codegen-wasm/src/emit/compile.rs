//! `WasmModuleEmitter::compile`: the single giant orchestration method that
//! drives every other helper to produce the final WASM binary + async-JS glue.
//!
//! Pure code-movement from `mod.rs`. Behaviour is unchanged.

use super::*;

impl WasmModuleEmitter {
    pub(super) fn compile(
        &mut self,
        modules: &[(String, perry_hir::ir::Module)],
    ) -> WasmCompileOutput {
        // First pass: collect all string literals
        for (_, module) in modules {
            self.collect_strings(module);
        }

        // Register runtime import types and get type indices
        // All imports use f64 for JSValues
        let t_void = self.get_type_idx(vec![], vec![]);
        let t_i32_i32_void = self.get_type_idx(vec![ValType::I32, ValType::I32], vec![]);
        let t_f64_void = self.get_type_idx(vec![ValType::I64], vec![]);
        let t_f64_f64_f64 = self.get_type_idx(vec![ValType::I64, ValType::I64], vec![ValType::I64]);
        let t_f64_f64_i32 = self.get_type_idx(vec![ValType::I64, ValType::I64], vec![ValType::I32]);
        let t_f64_f64 = self.get_type_idx(vec![ValType::I64], vec![ValType::I64]);
        let t_f64_i32 = self.get_type_idx(vec![ValType::I64], vec![ValType::I32]);
        let t_void_f64 = self.get_type_idx(vec![], vec![ValType::I64]);

        // Add runtime imports (order matters — defines function indices)
        let mut import_idx: u32 = 0;
        let mut next_import = || {
            let i = import_idx;
            import_idx += 1;
            i
        };

        // Additional type signatures needed for Phase 1+
        let t_f64_f64_void = self.get_type_idx(vec![ValType::I64, ValType::I64], vec![]);
        let t_f64_f64_f64_void =
            self.get_type_idx(vec![ValType::I64, ValType::I64, ValType::I64], vec![]);
        let t_f64_f64_f64_f64 = self.get_type_idx(
            vec![ValType::I64, ValType::I64, ValType::I64],
            vec![ValType::I64],
        );
        let t_f64_f64_f64_f64_f64 = self.get_type_idx(
            vec![ValType::I64, ValType::I64, ValType::I64, ValType::I64],
            vec![ValType::I64],
        );

        let rt = RuntimeImports {
            string_new: next_import(),
            console_log: next_import(),
            console_warn: next_import(),
            console_error: next_import(),
            string_concat: next_import(),
            js_add: next_import(),
            string_eq: next_import(),
            string_len: next_import(),
            jsvalue_to_string: next_import(),
            is_truthy: next_import(),
            js_strict_eq: next_import(),
            math_floor: next_import(),
            math_ceil: next_import(),
            math_round: next_import(),
            math_abs: next_import(),
            math_sqrt: next_import(),
            math_pow: next_import(),
            math_random: next_import(),
            math_log: next_import(),
            date_now: next_import(),
            js_typeof: next_import(),
            math_min: next_import(),
            math_max: next_import(),
            parse_int: next_import(),
            parse_float: next_import(),
            // Phase 0
            js_mod: next_import(),
            is_null_or_undefined: next_import(),
            // Phase 1: Objects
            object_new: next_import(),
            object_set: next_import(),
            object_get: next_import(),
            object_get_dynamic: next_import(),
            object_set_dynamic: next_import(),
            object_delete: next_import(),
            object_delete_dynamic: next_import(),
            object_keys: next_import(),
            object_values: next_import(),
            object_entries: next_import(),
            object_has_property: next_import(),
            object_assign: next_import(),
            // Phase 1: Arrays
            array_new: next_import(),
            array_push: next_import(),
            array_pop: next_import(),
            array_get: next_import(),
            array_set: next_import(),
            array_length: next_import(),
            array_slice: next_import(),
            array_splice: next_import(),
            array_shift: next_import(),
            array_unshift: next_import(),
            array_join: next_import(),
            array_index_of: next_import(),
            array_includes: next_import(),
            array_concat: next_import(),
            array_reverse: next_import(),
            array_flat: next_import(),
            array_is_array: next_import(),
            array_from: next_import(),
            array_push_spread: next_import(),
            // Phase 1: Strings
            string_char_at: next_import(),
            string_substring: next_import(),
            string_index_of: next_import(),
            string_slice: next_import(),
            string_to_lower_case: next_import(),
            string_to_upper_case: next_import(),
            string_trim: next_import(),
            string_includes: next_import(),
            string_starts_with: next_import(),
            string_ends_with: next_import(),
            string_replace: next_import(),
            string_split: next_import(),
            string_from_char_code: next_import(),
            string_pad_start: next_import(),
            string_pad_end: next_import(),
            string_repeat: next_import(),
            string_match: next_import(),
            math_log2: next_import(),
            math_log10: next_import(),
            // Phase 2: Closures
            closure_new: next_import(),
            closure_set_capture: next_import(),
            closure_call_0: next_import(),
            closure_call_1: next_import(),
            closure_call_2: next_import(),
            closure_call_3: next_import(),
            closure_call_spread: next_import(),
            // Phase 2: Array higher-order
            array_map: next_import(),
            array_filter: next_import(),
            array_for_each: next_import(),
            array_reduce: next_import(),
            array_find: next_import(),
            array_find_index: next_import(),
            array_sort: next_import(),
            array_some: next_import(),
            array_every: next_import(),
            // Phase 3: Classes
            class_new: next_import(),
            class_set_method: next_import(),
            class_call_method: next_import(),
            class_get_field: next_import(),
            class_set_field: next_import(),
            class_set_static: next_import(),
            class_get_static: next_import(),
            class_instanceof: next_import(),
            // Phase 4: JSON
            json_parse: next_import(),
            json_stringify: next_import(),
            // Phase 4: Map
            map_new: next_import(),
            map_set: next_import(),
            map_get: next_import(),
            map_has: next_import(),
            map_delete: next_import(),
            map_size: next_import(),
            map_clear: next_import(),
            map_entries: next_import(),
            map_keys: next_import(),
            map_values: next_import(),
            // Phase 4: Set
            set_new: next_import(),
            set_new_from_array: next_import(),
            set_add: next_import(),
            set_has: next_import(),
            set_delete: next_import(),
            set_size: next_import(),
            set_clear: next_import(),
            set_values: next_import(),
            // Phase 4: Date
            date_new: next_import(),
            date_get_time: next_import(),
            date_to_iso_string: next_import(),
            date_get_full_year: next_import(),
            date_get_month: next_import(),
            date_get_date: next_import(),
            date_get_day: next_import(),
            date_get_hours: next_import(),
            date_get_minutes: next_import(),
            date_get_seconds: next_import(),
            date_get_milliseconds: next_import(),
            // Phase 4: Error
            error_new: next_import(),
            error_message: next_import(),
            // Phase 4: RegExp
            regexp_new: next_import(),
            regexp_test: next_import(),
            // Phase 4: Globals
            number_coerce: next_import(),
            is_nan: next_import(),
            is_finite: next_import(),
            // Phase 5: Misc
            console_log_multi: next_import(),
            // Phase 1 addition: Class inheritance
            class_set_parent: next_import(),
            // Phase 3: Try/Catch
            try_start: next_import(),
            try_end: next_import(),
            throw_value: next_import(),
            has_exception: next_import(),
            get_exception: next_import(),
            // Phase 4: URL
            url_parse: next_import(),
            url_get_href: next_import(),
            url_get_pathname: next_import(),
            url_get_hostname: next_import(),
            url_get_port: next_import(),
            url_get_search: next_import(),
            url_get_hash: next_import(),
            url_get_origin: next_import(),
            url_get_protocol: next_import(),
            url_get_search_params: next_import(),
            searchparams_get: next_import(),
            searchparams_has: next_import(),
            searchparams_set: next_import(),
            searchparams_append: next_import(),
            searchparams_delete: next_import(),
            searchparams_to_string: next_import(),
            // Phase 4: Crypto
            crypto_random_uuid: next_import(),
            crypto_random_bytes: next_import(),
            // Phase 4: Path
            path_join: next_import(),
            path_dirname: next_import(),
            path_basename: next_import(),
            path_extname: next_import(),
            path_resolve: next_import(),
            // Phase 4: Process/OS
            os_platform: next_import(),
            process_argv: next_import(),
            process_cwd: next_import(),
            // Phase 6: Buffer
            buffer_alloc: next_import(),
            buffer_from_string: next_import(),
            buffer_to_string: next_import(),
            buffer_get: next_import(),
            buffer_set: next_import(),
            buffer_length: next_import(),
            buffer_slice: next_import(),
            buffer_concat: next_import(),
            uint8array_new: next_import(),
            uint8array_from: next_import(),
            uint8array_length: next_import(),
            uint8array_get: next_import(),
            uint8array_set: next_import(),
            // Timers
            set_timeout: next_import(),
            set_interval: next_import(),
            clear_timeout: next_import(),
            clear_interval: next_import(),
            // Response properties
            response_status: next_import(),
            response_ok: next_import(),
            response_headers_get: next_import(),
            response_url: next_import(),
            // Buffer extras
            buffer_copy: next_import(),
            buffer_write: next_import(),
            buffer_equals: next_import(),
            buffer_is_buffer: next_import(),
            buffer_byte_length: next_import(),
            // Crypto extras
            crypto_sha256: next_import(),
            crypto_md5: next_import(),
            // Path extras
            path_is_absolute: next_import(),
            // Phase 5: Async/Promise/Fetch
            fetch_url: next_import(),
            fetch_with_options: next_import(),
            response_json: next_import(),
            response_text: next_import(),
            promise_new: next_import(),
            promise_resolve: next_import(),
            promise_then: next_import(),
            await_promise: next_import(),
            // Memory-based bridge (Firefox NaN canonicalization workaround)
            mem_call: next_import(),
            mem_call_i32: next_import(),
        };
        self.num_imports = import_idx;
        self.rt = Some(rt);

        // Additional types for new phases
        let t_void_i32 = self.get_type_idx(vec![], vec![ValType::I32]);

        // Build import tables dynamically from struct fields
        // Each entry: (name, type_idx)
        let import_entries: Vec<(&str, u32)> = vec![
            ("string_new", t_i32_i32_void),
            ("console_log", t_f64_void),
            ("console_warn", t_f64_void),
            ("console_error", t_f64_void),
            ("string_concat", t_f64_f64_f64),
            ("js_add", t_f64_f64_f64),
            ("string_eq", t_f64_f64_i32),
            ("string_len", t_f64_f64),
            ("jsvalue_to_string", t_f64_f64),
            ("is_truthy", t_f64_i32),
            ("js_strict_eq", t_f64_f64_i32),
            ("math_floor", t_f64_f64),
            ("math_ceil", t_f64_f64),
            ("math_round", t_f64_f64),
            ("math_abs", t_f64_f64),
            ("math_sqrt", t_f64_f64),
            ("math_pow", t_f64_f64_f64),
            ("math_random", t_void_f64),
            ("math_log", t_f64_f64),
            ("date_now", t_void_f64),
            ("js_typeof", t_f64_f64),
            ("math_min", t_f64_f64_f64),
            ("math_max", t_f64_f64_f64),
            ("parse_int", t_f64_f64),
            ("parse_float", t_f64_f64),
            // Phase 0
            ("js_mod", t_f64_f64_f64),
            ("is_null_or_undefined", t_f64_i32),
            // Phase 1: Objects (f64 handles)
            ("object_new", t_void_f64),                 // () -> handle
            ("object_set", t_f64_f64_f64_f64), // (handle, key_str, value) -> handle (chaining)
            ("object_get", t_f64_f64_f64),     // (handle, key_str) -> value
            ("object_get_dynamic", t_f64_f64_f64), // (handle, key) -> value
            ("object_set_dynamic", t_f64_f64_f64_void), // (handle, key, value) -> void
            ("object_delete", t_f64_f64_void), // (handle, key_str) -> void
            ("object_delete_dynamic", t_f64_f64_void), // (handle, key) -> void
            ("object_keys", t_f64_f64),        // (handle) -> array_handle
            ("object_values", t_f64_f64),      // (handle) -> array_handle
            ("object_entries", t_f64_f64),     // (handle) -> array_handle
            ("object_has_property", t_f64_f64_i32), // (handle, key) -> i32
            ("object_assign", t_f64_f64_f64),  // (target, source) -> target
            // Phase 1: Arrays
            ("array_new", t_void_f64),            // () -> handle
            ("array_push", t_f64_f64_f64),        // (handle, value) -> handle (chaining)
            ("array_pop", t_f64_f64),             // (handle) -> value
            ("array_get", t_f64_f64_f64),         // (handle, index) -> value
            ("array_set", t_f64_f64_f64_void),    // (handle, index, value) -> void
            ("array_length", t_f64_f64),          // (handle) -> length
            ("array_slice", t_f64_f64_f64_f64),   // (handle, start, end) -> new_handle
            ("array_splice", t_f64_f64_f64_f64),  // (handle, start, deleteCount) -> removed_handle
            ("array_shift", t_f64_f64),           // (handle) -> value
            ("array_unshift", t_f64_f64_void),    // (handle, value) -> void
            ("array_join", t_f64_f64_f64),        // (handle, separator) -> string
            ("array_index_of", t_f64_f64_f64),    // (handle, value) -> index
            ("array_includes", t_f64_f64_i32),    // (handle, value) -> i32
            ("array_concat", t_f64_f64_f64),      // (handle1, handle2) -> new_handle
            ("array_reverse", t_f64_f64),         // (handle) -> handle
            ("array_flat", t_f64_f64),            // (handle) -> new_handle
            ("array_is_array", t_f64_i32),        // (value) -> i32
            ("array_from", t_f64_f64),            // (value) -> handle
            ("array_push_spread", t_f64_f64_f64), // (target, source) -> handle (chaining)
            // Phase 1: Strings
            ("string_charAt", t_f64_f64_f64), // (str, idx) -> str
            ("string_substring", t_f64_f64_f64_f64), // (str, start, end) -> str
            ("string_indexOf", t_f64_f64_f64), // (str, search) -> number
            ("string_slice", t_f64_f64_f64_f64), // (str, start, end) -> str
            ("string_toLowerCase", t_f64_f64),
            ("string_toUpperCase", t_f64_f64),
            ("string_trim", t_f64_f64),
            ("string_includes", t_f64_f64_i32),
            ("string_startsWith", t_f64_f64_i32),
            ("string_endsWith", t_f64_f64_i32),
            ("string_replace", t_f64_f64_f64_f64), // (str, pat, repl) -> str
            ("string_split", t_f64_f64_f64),       // (str, delim) -> array_handle
            ("string_fromCharCode", t_f64_f64),    // (code) -> str
            ("string_padStart", t_f64_f64_f64_f64), // (str, len, fill) -> str
            ("string_padEnd", t_f64_f64_f64_f64),
            ("string_repeat", t_f64_f64_f64), // (str, count) -> str
            ("string_match", t_f64_f64_f64),  // (str, regex) -> array_handle
            ("math_log2", t_f64_f64),
            ("math_log10", t_f64_f64),
            // Phase 2: Closures
            ("closure_new", t_f64_f64_f64), // (func_table_idx, capture_count) -> handle
            ("closure_set_capture", t_f64_f64_f64_f64), // (handle, idx, value) -> handle (chaining)
            ("closure_call_0", t_f64_f64),  // (handle) -> result
            ("closure_call_1", t_f64_f64_f64), // (handle, arg0) -> result
            ("closure_call_2", t_f64_f64_f64_f64), // (handle, arg0, arg1) -> result
            ("closure_call_3", t_f64_f64_f64_f64_f64), // (handle, arg0, arg1, arg2) -> result
            ("closure_call_spread", t_f64_f64_f64), // (handle, args_array) -> result
            // Phase 2: Array higher-order
            ("array_map", t_f64_f64_f64), // (handle, closure) -> new_handle
            ("array_filter", t_f64_f64_f64),
            ("array_forEach", t_f64_f64_void), // (handle, closure) -> void
            ("array_reduce", t_f64_f64_f64_f64), // (handle, closure, initial) -> value
            ("array_find", t_f64_f64_f64),     // (handle, closure) -> value
            ("array_find_index", t_f64_f64_f64), // (handle, closure) -> number
            ("array_sort", t_f64_f64_f64),     // (handle, closure) -> handle
            ("array_some", t_f64_f64_i32),     // (handle, closure) -> i32
            ("array_every", t_f64_f64_i32),    // (handle, closure) -> i32
            // Phase 3: Classes
            ("class_new", t_f64_f64_f64), // (class_id, field_count) -> handle
            ("class_set_method", t_f64_f64_f64_void), // (class_id, name_str, func_table_idx) -> void
            ("class_call_method", t_f64_f64_f64_f64), // (handle, name_str, args_array) -> result
            ("class_get_field", t_f64_f64_f64),       // (handle, name_str) -> value
            ("class_set_field", t_f64_f64_f64_void),  // (handle, name_str, value) -> void
            ("class_set_static", t_f64_f64_f64_void), // (class_id, name_str, value) -> void
            ("class_get_static", t_f64_f64_f64),      // (class_id, name_str) -> value
            ("class_instanceof", t_f64_f64_i32),      // (handle, class_id) -> i32
            // Phase 4: JSON
            ("json_parse", t_f64_f64),     // (str) -> handle
            ("json_stringify", t_f64_f64), // (value) -> str
            // Phase 4: Map
            ("map_new", t_void_f64),
            ("map_set", t_f64_f64_f64_void), // (handle, key, value) -> void
            ("map_get", t_f64_f64_f64),
            ("map_has", t_f64_f64_i32),
            ("map_delete", t_f64_f64_void),
            ("map_size", t_f64_f64),
            ("map_clear", t_f64_void),
            ("map_entries", t_f64_f64),
            ("map_keys", t_f64_f64),
            ("map_values", t_f64_f64),
            // Phase 4: Set
            ("set_new", t_void_f64),
            ("set_new_from_array", t_f64_f64),
            ("set_add", t_f64_f64_void),
            ("set_has", t_f64_f64_i32),
            ("set_delete", t_f64_f64_void),
            ("set_size", t_f64_f64),
            ("set_clear", t_f64_void),
            ("set_values", t_f64_f64),
            // Phase 4: Date
            ("date_new_val", t_f64_f64), // (opt_arg) -> handle
            ("date_get_time", t_f64_f64),
            ("date_to_iso_string", t_f64_f64),
            ("date_get_full_year", t_f64_f64),
            ("date_get_month", t_f64_f64),
            ("date_get_date", t_f64_f64),
            ("date_get_day", t_f64_f64),
            ("date_get_hours", t_f64_f64),
            ("date_get_minutes", t_f64_f64),
            ("date_get_seconds", t_f64_f64),
            ("date_get_milliseconds", t_f64_f64),
            // Phase 4: Error
            ("error_new", t_f64_f64),     // (message) -> handle
            ("error_message", t_f64_f64), // (handle) -> string
            // Phase 4: RegExp
            ("regexp_new", t_f64_f64_f64), // (pattern, flags) -> handle
            ("regexp_test", t_f64_f64_i32), // (regex, str) -> i32
            // Phase 4: Globals
            ("number_coerce", t_f64_f64),
            ("is_nan", t_f64_i32),
            ("is_finite", t_f64_i32),
            // Phase 5
            ("console_log_multi", t_f64_void), // (args_array) -> void
            // Phase 1 addition: Class inheritance
            ("class_set_parent", t_f64_f64_void), // (child_str, parent_str) -> void
            // Phase 3: Try/Catch
            ("try_start", t_void),         // () -> void
            ("try_end", t_void),           // () -> void
            ("throw_value", t_f64_void),   // (val) -> void
            ("has_exception", t_void_i32), // () -> i32
            ("get_exception", t_void_f64), // () -> f64
            // Phase 4: URL
            ("url_parse", t_f64_f64), // (url_str) -> handle
            ("url_get_href", t_f64_f64),
            ("url_get_pathname", t_f64_f64),
            ("url_get_hostname", t_f64_f64),
            ("url_get_port", t_f64_f64),
            ("url_get_search", t_f64_f64),
            ("url_get_hash", t_f64_f64),
            ("url_get_origin", t_f64_f64),
            ("url_get_protocol", t_f64_f64),
            ("url_get_search_params", t_f64_f64),
            ("searchparams_get", t_f64_f64_f64), // (handle, key) -> str
            ("searchparams_has", t_f64_f64_i32), // (handle, key) -> i32
            ("searchparams_set", t_f64_f64_f64_void), // (handle, key, val) -> void
            ("searchparams_append", t_f64_f64_f64_void),
            ("searchparams_delete", t_f64_f64_void),
            ("searchparams_to_string", t_f64_f64),
            // Phase 4: Crypto
            ("crypto_random_uuid", t_void_f64),
            ("crypto_random_bytes", t_f64_f64),
            // Phase 4: Path
            ("path_join", t_f64_f64_f64), // (a, b) -> str
            ("path_dirname", t_f64_f64),
            ("path_basename", t_f64_f64),
            ("path_extname", t_f64_f64),
            ("path_resolve", t_f64_f64),
            // Phase 4: Process/OS
            ("os_platform", t_void_f64),
            ("process_argv", t_void_f64),
            ("process_cwd", t_void_f64),
            // Phase 6: Buffer
            ("buffer_alloc", t_f64_f64),
            ("buffer_from_string", t_f64_f64_f64),
            ("buffer_to_string", t_f64_f64_f64),
            ("buffer_get", t_f64_f64_f64),
            ("buffer_set", t_f64_f64_f64_void),
            ("buffer_length", t_f64_f64),
            ("buffer_slice", t_f64_f64_f64_f64),
            ("buffer_concat", t_f64_f64),
            ("uint8array_new", t_f64_f64),
            ("uint8array_from", t_f64_f64),
            ("uint8array_length", t_f64_f64),
            ("uint8array_get", t_f64_f64_f64),
            ("uint8array_set", t_f64_f64_f64_void),
            // Timers
            ("set_timeout", t_f64_f64_f64), // (closure, delay) -> timer_id
            ("set_interval", t_f64_f64_f64), // (closure, delay) -> timer_id
            ("clear_timeout", t_f64_void),  // (id) -> void
            ("clear_interval", t_f64_void), // (id) -> void
            // Response properties
            ("response_status", t_f64_f64), // (handle) -> number
            ("response_ok", t_f64_i32),     // (handle) -> i32
            ("response_headers_get", t_f64_f64_f64), // (handle, name) -> str
            ("response_url", t_f64_f64),    // (handle) -> str
            // Buffer extras
            ("buffer_copy", {
                self.get_type_idx(vec![ValType::I64; 5], vec![ValType::I64])
            }),
            ("buffer_write", t_f64_f64_f64_f64), // (handle, str, offset, encoding) -> number
            ("buffer_equals", t_f64_f64_i32),    // (handle, other) -> i32
            ("buffer_is_buffer", t_f64_i32),     // (val) -> i32
            ("buffer_byte_length", t_f64_f64),   // (val) -> number
            // Crypto extras
            ("crypto_sha256", t_f64_f64), // (data) -> promise_handle
            ("crypto_md5", t_f64_f64),    // (data) -> undefined
            // Path extras
            ("path_is_absolute", t_f64_i32), // (str) -> i32
            // Phase 5: Async/Promise/Fetch
            ("fetch_url", t_f64_f64), // (url_str) -> promise_handle
            ("fetch_with_options", t_f64_f64_f64_f64), // (url, method, body, headers_obj) -> promise_handle
            ("response_json", t_f64_f64),              // (response_handle) -> promise_handle
            ("response_text", t_f64_f64),              // (response_handle) -> promise_handle
            ("promise_new", t_void_f64),               // () -> promise_handle
            ("promise_resolve", t_f64_f64_void),       // (promise_handle, value) -> void
            ("promise_then", t_f64_f64_f64), // (promise_handle, closure_handle) -> promise_handle
            ("await_promise", t_f64_f64),    // (value) -> resolved_value_or_value
            // Memory-based bridge: args written to WASM memory at 0xFF00, only plain numbers as params
            ("mem_call", {
                self.get_type_idx(
                    vec![ValType::F64, ValType::F64, ValType::I32],
                    vec![ValType::F64],
                )
            }), // (func_name_id, arg_count, base_addr) -> f64 dummy
            ("mem_call_i32", {
                self.get_type_idx(
                    vec![ValType::F64, ValType::F64, ValType::I32],
                    vec![ValType::I32],
                )
            }), // (func_name_id, arg_count, base_addr) -> i32
        ];

        // Collect all closures from all modules (they need function indices too).
        // Track the module index so closures can be associated with their parent module's func_map.
        let mut closure_funcs: Vec<(
            FuncId,
            Vec<Param>,
            Vec<Stmt>,
            Vec<LocalId>,
            Vec<LocalId>,
            usize,
        )> = Vec::new();
        for (mod_idx, (_, module)) in modules.iter().enumerate() {
            let mut module_closures: Vec<(
                FuncId,
                Vec<Param>,
                Vec<Stmt>,
                Vec<LocalId>,
                Vec<LocalId>,
            )> = Vec::new();
            collect_closures_from_stmts(&module.init, &mut module_closures);
            for func in &module.functions {
                collect_closures_from_stmts(&func.body, &mut module_closures);
            }
            for class in &module.classes {
                if let Some(ctor) = &class.constructor {
                    collect_closures_from_stmts(&ctor.body, &mut module_closures);
                }
                for method in &class.methods {
                    collect_closures_from_stmts(&method.body, &mut module_closures);
                }
                for method in &class.static_methods {
                    collect_closures_from_stmts(&method.body, &mut module_closures);
                }
                for (_, getter) in &class.getters {
                    collect_closures_from_stmts(&getter.body, &mut module_closures);
                }
                for (_, setter) in &class.setters {
                    collect_closures_from_stmts(&setter.body, &mut module_closures);
                }
                for field in &class.fields {
                    if let Some(init) = &field.init {
                        collect_closures_from_expr(init, &mut module_closures);
                    }
                }
                for field in &class.static_fields {
                    if let Some(init) = &field.init {
                        collect_closures_from_expr(init, &mut module_closures);
                    }
                }
            }
            for (fid, params, body, caps, mut_caps) in module_closures {
                closure_funcs.push((fid, params, body, caps, mut_caps, mod_idx));
            }
        }

        // Register async functions as additional bridge imports (Phase 1: assign import indices).
        // JS code generation is deferred to Phase 2 after per-module func_maps are built.
        let mut async_import_idx = self.num_imports;
        let mut per_module_async: Vec<Vec<(FuncId, u32)>> = Vec::new();
        for (_, module) in modules.iter() {
            let mut module_async_entries = Vec::new();
            for func in &module.functions {
                if func.is_async {
                    let param_count = func.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64]; // returns promise handle
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    module_async_entries.push((func.id, async_import_idx));
                    self.func_name_map
                        .insert(func.name.clone(), async_import_idx);
                    // Record the param count so optional-arg padding at call sites
                    // (expr.rs Expr::Call → FuncRef/ExternFuncRef) works for async
                    // imports too. Without this, `await foo(a, b)` where foo is
                    // declared as `async function foo(a, b, c?)` emits only two
                    // i64 pushes before the call, while the import is declared as
                    // `(i64, i64, i64) -> i64`. Validator fails with
                    // "expected i64 but nothing on stack" (#1081 sibling instance).
                    self.func_param_counts.insert(async_import_idx, param_count);
                    self.async_func_imports.push((
                        func.name.clone(),
                        async_import_idx,
                        param_count,
                    ));
                    async_import_idx += 1;
                }
            }
            per_module_async.push(module_async_entries);
        }
        self.num_imports = async_import_idx;

        // Register external FFI functions as WASM imports under the "ffi" namespace.
        // These are `declare function` statements with no body (e.g., bloom_init_window).
        // Deduplicate by name since the same extern can appear in multiple modules.
        let mut ffi_import_idx = self.num_imports;
        let mut seen_ffi: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (_, module) in modules {
            for (name, param_types, return_type) in &module.extern_funcs {
                if seen_ffi.contains(name) {
                    continue;
                }
                seen_ffi.insert(name.clone());
                let param_count = param_types.len();
                let has_return = !matches!(return_type, perry_hir::types::Type::Void);
                let params = vec![ValType::I64; param_count];
                let results = if has_return {
                    vec![ValType::I64]
                } else {
                    vec![]
                };
                let type_idx = self.get_type_idx(params, results);
                let _ = type_idx;
                self.func_name_map.insert(name.clone(), ffi_import_idx);
                self.ffi_imports
                    .push((name.clone(), param_count, has_return));
                ffi_import_idx += 1;
            }
        }
        self.num_imports = ffi_import_idx;

        // Now set user_func_idx AFTER all imports (including async and FFI) are registered
        let mut user_func_idx = self.num_imports;

        // __init_strings function
        let init_strings_idx = user_func_idx;
        let init_strings_type = t_void;
        user_func_idx += 1;

        // Register user functions from all modules (skip async ones).
        // FuncId is only unique within a module, so we build per-module func_maps
        // to avoid cross-module FuncId collisions (e.g., module A's FuncId(2) != module B's FuncId(2)).
        for (mod_idx, (_, module)) in modules.iter().enumerate() {
            let mut module_fm: BTreeMap<FuncId, u32> = BTreeMap::new();
            // Include async function mappings for this module
            for &(fid, idx) in &per_module_async[mod_idx] {
                module_fm.insert(fid, idx);
            }
            // Function names are NOT globally unique across modules (a
            // serializer's local `function vec3(v): string` coexists with the
            // math library's exported `vec3(x, y, z)`), but `func_name_map` —
            // the ExternFuncRef cross-module resolution table — is keyed by
            // bare name. Prefer EXPORTED functions (the only legitimate
            // cross-module call targets); a module-local helper only claims a
            // name nobody exported.
            let exported_names: std::collections::HashSet<&str> = module
                .exported_functions
                .iter()
                .map(|(n, _)| n.as_str())
                .chain(module.exports.iter().filter_map(|e| match e {
                    perry_hir::ir::Export::Named { local, .. } => Some(local.as_str()),
                    _ => None,
                }))
                .collect();
            for func in &module.functions {
                if func.is_async {
                    continue; // already registered as bridge import
                }
                let param_count = func.params.len();
                let params = vec![ValType::I64; param_count];
                let results = if func.body.iter().any(has_return) || func.name == "main" {
                    vec![ValType::I64]
                } else {
                    vec![]
                };
                let is_void = results.is_empty();
                let type_idx = self.get_type_idx(params, results);
                let _ = type_idx;
                module_fm.insert(func.id, user_func_idx);
                if is_void {
                    self.void_funcs.insert(user_func_idx);
                }
                self.func_param_counts.insert(user_func_idx, param_count);
                // Build func_name_map for ExternFuncRef resolution. Exported
                // functions win the name; module-local helpers only fill a
                // vacant slot (see exported_names above).
                if exported_names.contains(func.name.as_str()) {
                    self.func_name_map.insert(func.name.clone(), user_func_idx);
                } else {
                    self.func_name_map
                        .entry(func.name.clone())
                        .or_insert(user_func_idx);
                }
                user_func_idx += 1;
            }
            self.module_func_maps.push(module_fm);
        }

        // Register class constructors, methods, and static methods
        for (_, module) in modules {
            for class in &module.classes {
                // Record parent class relationship
                if let Some(parent) = &class.extends_name {
                    self.class_parent_map
                        .insert(class.name.clone(), parent.clone());
                }
                // Constructor: params = this + declared params, returns f64 (this)
                if let Some(ctor) = &class.constructor {
                    let param_count = 1 + ctor.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_ctor_map
                        .insert(class.name.clone(), user_func_idx);
                    self.func_param_counts.insert(user_func_idx, param_count);
                    user_func_idx += 1;
                }
                // Instance methods: params = this + declared params
                for method in &class.methods {
                    let param_count = 1 + method.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_method_map
                        .entry(class.name.clone())
                        .or_default()
                        .insert(method.name.clone(), user_func_idx);
                    self.func_param_counts.insert(user_func_idx, param_count);
                    user_func_idx += 1;
                }
                // Static methods: no this param
                for method in &class.static_methods {
                    let param_count = method.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_static_map
                        .entry(class.name.clone())
                        .or_default()
                        .insert(method.name.clone(), user_func_idx);
                    // Also register in func_name_map for cross-module resolution
                    self.func_name_map
                        .insert(format!("{}_{}", class.name, method.name), user_func_idx);
                    self.func_param_counts.insert(user_func_idx, param_count);
                    user_func_idx += 1;
                }
                // Getters: like methods with 0 params + this
                for (name, getter) in &class.getters {
                    let params = vec![ValType::I64]; // just this
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_method_map
                        .entry(class.name.clone())
                        .or_default()
                        .insert(format!("__get_{}", name), user_func_idx);
                    self.func_param_counts.insert(user_func_idx, 1);
                    let _ = getter;
                    user_func_idx += 1;
                }
                // Setters: this + value
                for (name, setter) in &class.setters {
                    let params = vec![ValType::I64; 2]; // this + value
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_method_map
                        .entry(class.name.clone())
                        .or_default()
                        .insert(format!("__set_{}", name), user_func_idx);
                    self.func_param_counts.insert(user_func_idx, 2);
                    let _ = setter;
                    user_func_idx += 1;
                }
            }
        }

        // Register closure functions into their per-module func_maps
        for (func_id, params, body, captures, mutable_captures, mod_idx) in &closure_funcs {
            if !self.module_func_maps[*mod_idx].contains_key(func_id) {
                // Closure params: captures first (as f64), then declared params
                let total_params = captures.len() + mutable_captures.len() + params.len();
                let wasm_params = vec![ValType::I64; total_params];
                let results = if body.iter().any(has_return) {
                    vec![ValType::I64]
                } else {
                    vec![ValType::I64] // closures always return i64
                };
                let type_idx = self.get_type_idx(wasm_params, results);
                let _ = type_idx;
                self.module_func_maps[*mod_idx].insert(*func_id, user_func_idx);
                user_func_idx += 1;
            }
        }

        // Async function JS generation (Phase 2): now that per-module func_maps are complete,
        // generate the JS code for async functions with correct FuncRef resolution.
        for (mod_idx, (_, module)) in modules.iter().enumerate() {
            self.func_map = self.module_func_maps[mod_idx].clone();
            for func in &module.functions {
                if func.is_async {
                    let js_code = self.emit_js_async_function(func);
                    self.async_js_code.push(js_code);
                }
            }
        }

        // _start function (entry point). #854: the trailing
        // `user_func_idx += 1` was dead — nothing later in this function
        // reads the counter.
        let start_idx = user_func_idx;
        let start_type = t_void;

        // Register globals from all modules
        for (_, module) in modules {
            for global in &module.globals {
                self.global_map.insert(global.id, self.num_globals);
                self.num_globals += 1;
            }
        }

        // Promote module-level Let bindings to WASM globals so cross-function
        // references work and so different modules' identical LocalIds don't collide.
        for (mod_idx, (_, module)) in modules.iter().enumerate() {
            collect_module_let_ids(
                &module.init,
                mod_idx,
                &mut self.module_let_globals,
                &mut self.num_globals,
            );
        }

        // Issue #1071: build cross-module imported-variable → WASM global map.
        // The HIR lowers `import { FOO } from './m'` value reads (where FOO is
        // an exported `const`/`let`, not a function) to `Expr::ExternFuncRef {
        // name: "FOO" }`. Pre-fix this hit `TAG_UNDEFINED` because `name`
        // wasn't in `func_name_map` (it's a variable, not a function). Now we
        // resolve `name` to the source module's `Stmt::Let` and reuse its
        // wasm-global slot from `module_let_globals`. Same flow the LLVM target
        // achieves via per-export `perry_fn_<src>__<name>()` getter functions.
        //
        // Module-path lookup: each `Import` carries `resolved_path` (set by
        // the driver) and `Module.name` is a relative-from-project-root path.
        // We compare paths by file-stem match against `Module.name` (which is
        // a leaf "name.ts" or "subdir/name.ts" string), falling back to a
        // basename match. Re-exports (`Export::ReExport`, `ExportAll`, and the
        // import-then-`export { x }` shape) are chased by
        // `resolve_export_to_let` with a depth cap — a library facade like
        // bloom's `index.ts` re-exporting `Key` from `core/keys.ts` is two to
        // three hops deep, and stopping at the first module made every
        // re-exported const OBJECT read undefined (scalars sometimes survived
        // via other paths, which made the failure look random).
        {
            // module.name → source module index
            let name_to_idx: std::collections::HashMap<&str, usize> = modules
                .iter()
                .enumerate()
                .map(|(i, (_, m))| (m.name.as_str(), i))
                .collect();
            // For each source module, build a name → wasm global lookup over
            // its top-level Lets so we can resolve `Export::Named { local }`.
            let mut src_let_names: Vec<std::collections::HashMap<String, u32>> =
                Vec::with_capacity(modules.len());
            for (src_idx, (_, module)) in modules.iter().enumerate() {
                let mut map: std::collections::HashMap<String, u32> = Default::default();
                for stmt in &module.init {
                    if let perry_hir::Stmt::Let { id, name, .. } = stmt {
                        if let Some(&gidx) = self.module_let_globals.get(&(src_idx, *id)) {
                            map.insert(name.clone(), gidx);
                        }
                    }
                }
                src_let_names.push(map);
            }
            for (consumer_idx, (_, module)) in modules.iter().enumerate() {
                for import in &module.imports {
                    if import.type_only {
                        continue;
                    }
                    // Resolve source module index. Prefer matching `resolved_path`
                    // against `(path, module)` pairs by stem; fall back to a
                    // suffix/basename match on `import.source`.
                    let src_idx_opt = resolve_source_module_idx(modules, import, &name_to_idx);
                    let Some(src_idx) = src_idx_opt else { continue };
                    for spec in &import.specifiers {
                        if let perry_hir::ir::ImportSpecifier::Named { imported, local } = spec {
                            // Resolve the public `imported` name to a let
                            // global, following re-export chains (see
                            // resolve_export_to_let).
                            let resolved = resolve_export_to_let(
                                modules,
                                &src_let_names,
                                &name_to_idx,
                                src_idx,
                                imported,
                                8,
                            );
                            if std::env::var("PERRY_WASM_DEBUG_IMPORTS").is_ok() {
                                eprintln!(
                                    "[wasm-imports] {} imports {{ {} }} from {} -> module #{} ({}) => {:?}",
                                    modules[consumer_idx].1.name,
                                    imported,
                                    import.source,
                                    src_idx,
                                    modules[src_idx].1.name,
                                    resolved,
                                );
                            }
                            if let Some(gidx) = resolved {
                                self.imported_var_globals
                                    .insert((consumer_idx, local.clone()), gidx);
                            }
                            // Function imports resolve per-consumer too — the
                            // whole-program func_name_map's bare-name keys
                            // collide across modules.
                            if let Some(fidx) = resolve_export_to_func(
                                modules,
                                &self.module_func_maps,
                                &name_to_idx,
                                src_idx,
                                imported,
                                8,
                            ) {
                                self.imported_func_indices
                                    .insert((consumer_idx, local.clone()), fidx);
                            }
                        }
                        // Namespace import (`import * as W from "./mod"`):
                        // register every exported module-level let under a
                        // DOTTED key ("W.MESH_COUNT"), so PropertyGet on the
                        // namespace ident resolves to the source module's
                        // promoted-let global — the same mechanism the Named
                        // arm above uses. Without this, every `W.member` read
                        // emitted a class_get_field on an undefined receiver
                        // and produced undefined (functions kept working via
                        // the whole-program name map, which made the failure
                        // maddeningly partial).
                        if let perry_hir::ir::ImportSpecifier::Namespace { local } = spec {
                            // Register `W.<member>` for exactly the source
                            // module's PUBLIC surface — its named/re-exported/
                            // function/object exports, plus everything reached
                            // through `export * from "..."` (recursively). This
                            // replaced a blanket "register every module-level
                            // let" loop, which both exposed PRIVATE locals as
                            // `W.private` (not valid JS namespace members) and
                            // missed `export *` re-exports entirely.
                            let mut public: std::collections::BTreeSet<String> =
                                std::collections::BTreeSet::new();
                            collect_exported_names(modules, src_idx, 8, &mut public);
                            for name in &public {
                                if let Some(gidx) = resolve_export_to_let(
                                    modules,
                                    &src_let_names,
                                    &name_to_idx,
                                    src_idx,
                                    name,
                                    8,
                                ) {
                                    self.imported_var_globals.insert(
                                        (consumer_idx, format!("{}.{}", local, name)),
                                        gidx,
                                    );
                                }
                                if let Some(fidx) = resolve_export_to_func(
                                    modules,
                                    &self.module_func_maps,
                                    &name_to_idx,
                                    src_idx,
                                    name,
                                    8,
                                ) {
                                    self.imported_ns_funcs
                                        .entry((consumer_idx, format!("{}.{}", local, name)))
                                        .or_insert(fidx);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Add a NaN-safe temp global for mem_store_slot (Firefox canonicalizes locals)
        self.nan_temp_global = self.num_globals;
        self.num_globals += 1;

        // Build the WASM module
        let mut wasm_module = Module::new();

        // --- Type section ---
        let mut type_section = TypeSection::new();
        for (params, results) in &self.types {
            type_section
                .ty()
                .function(params.iter().copied(), results.iter().copied());
        }
        wasm_module.section(&type_section);

        // --- Import section ---
        let mut import_section = ImportSection::new();
        for (name, type_idx) in &import_entries {
            import_section.import("rt", name, EntityType::Function(*type_idx));
        }
        // Add async function imports
        let async_import_entries: Vec<(String, u32)> = self
            .async_func_imports
            .iter()
            .map(|(name, _idx, param_count)| {
                let import_name = format!("__async_{}", name);
                let params = vec![ValType::I64; *param_count];
                let results = vec![ValType::I64];
                let key = (params, results);
                let type_idx = self.type_map.get(&key).copied().unwrap_or(0);
                (import_name, type_idx)
            })
            .collect();
        for (name, type_idx) in &async_import_entries {
            import_section.import("rt", name, EntityType::Function(*type_idx));
        }
        // Add FFI function imports under "ffi" namespace
        for (name, param_count, has_return) in &self.ffi_imports {
            let params = vec![ValType::I64; *param_count];
            let results = if *has_return {
                vec![ValType::I64]
            } else {
                vec![]
            };
            let key = (params, results);
            let type_idx = self.type_map.get(&key).copied().unwrap_or(0);
            import_section.import("ffi", name, EntityType::Function(type_idx));
        }
        wasm_module.section(&import_section);

        // --- Function section (declares type indices for each defined function) ---
        let mut func_section = FunctionSection::new();
        // __init_strings
        func_section.function(init_strings_type);
        // User functions (skip async — they are imports)
        for (_, module) in modules {
            for func in &module.functions {
                if func.is_async {
                    continue;
                }
                let param_count = func.params.len();
                let params = vec![ValType::I64; param_count];
                let results = if func.body.iter().any(has_return) || func.name == "main" {
                    vec![ValType::I64]
                } else {
                    vec![]
                };
                let type_idx = self.get_type_idx(params, results);
                func_section.function(type_idx);
            }
        }
        // Class constructors, methods, static methods, getters, setters
        for (_, module) in modules {
            for class in &module.classes {
                if let Some(ctor) = &class.constructor {
                    let param_count = 1 + ctor.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
                for method in &class.methods {
                    let param_count = 1 + method.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
                for method in &class.static_methods {
                    let param_count = method.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
                for (_name, _getter) in &class.getters {
                    let params = vec![ValType::I64];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
                for (_name, _setter) in &class.setters {
                    let params = vec![ValType::I64; 2];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
            }
        }
        // Closure functions
        for (func_id, _params, _body, captures, mutable_captures, mod_idx) in &closure_funcs {
            if self.module_func_maps[*mod_idx].contains_key(func_id) {
                let total_params = captures.len() + mutable_captures.len() + _params.len();
                let wasm_params = vec![ValType::I64; total_params];
                let results = vec![ValType::I64]; // closures always return f64
                let type_idx = self.get_type_idx(wasm_params, results);
                func_section.function(type_idx);
            }
        }
        // _start
        func_section.function(start_type);
        wasm_module.section(&func_section);

        // --- Table section (for indirect calls / closures) ---
        // Must come after Function section but before Memory section (WASM spec ordering)
        let all_func_indices: Vec<u32> = {
            let mut indices = vec![init_strings_idx]; // placeholder at index 0
            for (mod_idx, (_, module)) in modules.iter().enumerate() {
                for func in &module.functions {
                    if let Some(&idx) = self.module_func_maps[mod_idx].get(&func.id) {
                        indices.push(idx);
                    }
                }
            }
            // Add class constructor/method/static indices
            for idx in self.class_ctor_map.values() {
                if !indices.contains(idx) {
                    indices.push(*idx);
                }
            }
            for methods in self.class_method_map.values() {
                for idx in methods.values() {
                    if !indices.contains(idx) {
                        indices.push(*idx);
                    }
                }
            }
            for statics in self.class_static_map.values() {
                for idx in statics.values() {
                    if !indices.contains(idx) {
                        indices.push(*idx);
                    }
                }
            }
            for (func_id, _, _, _, _, mod_idx) in &closure_funcs {
                if let Some(&idx) = self.module_func_maps[*mod_idx].get(func_id) {
                    if !indices.contains(&idx) {
                        indices.push(idx);
                    }
                }
            }
            indices.push(start_idx);
            indices
        };
        // Build reverse map: wasm func index → table position
        for (table_idx, &func_idx) in all_func_indices.iter().enumerate() {
            self.func_to_table_idx.insert(func_idx, table_idx as u32);
        }

        let table_size = all_func_indices.len() as u32;
        {
            let mut table_section = TableSection::new();
            table_section.table(TableType {
                element_type: RefType::FUNCREF,
                minimum: table_size as u64,
                maximum: Some(table_size as u64),
                table64: false,
                shared: false,
            });
            wasm_module.section(&table_section);
        }

        // --- Memory section ---
        let mut mem_section = MemorySection::new();
        let pages = self.string_data.len().div_ceil(65536).max(2) as u64; // min 2 pages for 0xFF00 mem_call region
        mem_section.memory(MemoryType {
            minimum: pages,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        wasm_module.section(&mem_section);

        // --- Global section ---
        if self.num_globals > 0 {
            let mut global_section = GlobalSection::new();
            for g in 0..self.num_globals {
                if g == self.nan_temp_global {
                    // Stack pointer for arg buffer (i32, initialized to 0x10000)
                    global_section.global(
                        GlobalType {
                            val_type: ValType::I32,
                            mutable: true,
                            shared: false,
                        },
                        &wasm_encoder::ConstExpr::i32_const(0x10000),
                    );
                } else {
                    // Regular i64 global for module-level variables (NaN-boxed)
                    global_section.global(
                        GlobalType {
                            val_type: ValType::I64,
                            mutable: true,
                            shared: false,
                        },
                        &wasm_encoder::ConstExpr::i64_const(TAG_UNDEFINED as i64),
                    );
                }
            }
            wasm_module.section(&global_section);
        }

        // --- Export section ---
        let mut export_section = ExportSection::new();
        export_section.export("_start", ExportKind::Func, start_idx);
        export_section.export("memory", ExportKind::Memory, 0);
        export_section.export("__indirect_function_table", ExportKind::Table, 0);
        // Export all user functions so async JS code can call them by index.
        for idx in self.num_imports..start_idx {
            export_section.export(&format!("__wasm_func_{}", idx), ExportKind::Func, idx);
        }
        // Issue #1071: export the globals that back cross-module imported
        // variables so the async/JS-context emission path (which can't issue
        // a `global.get` instruction) can read them via
        // `wasmInstance.exports.__wasm_global_<idx>.value`. We export ALL
        // module-let globals since they're already named by index and the
        // export cost is negligible; future asynchrony work that needs the
        // same boundary read won't have to wire a separate index.
        {
            let mut exported_globals: std::collections::BTreeSet<u32> =
                std::collections::BTreeSet::new();
            for &gidx in self.module_let_globals.values() {
                exported_globals.insert(gidx);
            }
            for &gidx in self.global_map.values() {
                exported_globals.insert(gidx);
            }
            for gidx in exported_globals {
                export_section.export(&format!("__wasm_global_{}", gidx), ExportKind::Global, gidx);
            }
        }
        wasm_module.section(&export_section);

        // --- Element section (populate the indirect call table) ---
        {
            let mut elem_section = ElementSection::new();
            elem_section.active(
                Some(0),                                // table index
                &wasm_encoder::ConstExpr::i32_const(0), // offset
                Elements::Functions(std::borrow::Cow::Borrowed(&all_func_indices)),
            );
            wasm_module.section(&elem_section);
        }

        // --- DataCount section (required before Code when Data section exists) ---
        if !self.string_data.is_empty() {
            wasm_module.section(&wasm_encoder::DataCountSection { count: 1 });
        }

        // --- Code section ---
        let mut code_section = CodeSection::new();

        // __init_strings: register all string literals with the JS runtime
        {
            let mut func = Function::new(vec![]);
            for (_content, offset, len) in &self.string_table {
                func.instruction(&Instruction::I32Const(*offset as i32));
                func.instruction(&Instruction::I32Const(*len as i32));
                func.instruction(&Instruction::Call(rt.string_new));
            }
            func.instruction(&Instruction::End);
            code_section.function(&func);
        }

        // User functions (skip async — they are JS bridge imports).
        // Swap in the per-module func_map so FuncRef(id) resolves correctly within each module.
        for (mod_idx, (_, module)) in modules.iter().enumerate() {
            self.func_map = self.module_func_maps[mod_idx].clone();
            self.current_mod_idx = mod_idx;
            for hir_func in &module.functions {
                if hir_func.is_async {
                    continue;
                }
                let func = self.compile_function(hir_func);
                code_section.function(&func);
            }
        }

        // Class constructors, methods, static methods, getters, setters
        for (mod_idx, (_, module)) in modules.iter().enumerate() {
            self.func_map = self.module_func_maps[mod_idx].clone();
            self.current_mod_idx = mod_idx;
            for class in &module.classes {
                if let Some(ctor) = &class.constructor {
                    let func = self.compile_class_constructor(class, ctor);
                    code_section.function(&func);
                }
                for method in &class.methods {
                    let func = self.compile_class_method(method);
                    code_section.function(&func);
                }
                for method in &class.static_methods {
                    // Static methods are declared as `(params) -> i64` in
                    // func_section (mod.rs:1701) unconditionally, so the
                    // body emitter must also assume i64-returning regardless
                    // of whether the body has an explicit `Stmt::Return`.
                    // Otherwise a static method that only throws (or only
                    // falls through) produces a WASM body whose `return`
                    // instructions leave an empty operand stack — V8 rejects
                    // with "expected i64 but nothing on stack".
                    let func = self.compile_function_with_signature(method, true);
                    code_section.function(&func);
                }
                for (_name, getter) in &class.getters {
                    let func = self.compile_class_method(getter);
                    code_section.function(&func);
                }
                for (_name, setter) in &class.setters {
                    let func = self.compile_class_method(setter);
                    code_section.function(&func);
                }
            }
        }

        // Closure functions — swap in the parent module's func_map for each closure
        for (func_id, params, body, captures, mutable_captures, mod_idx) in &closure_funcs {
            if self.module_func_maps[*mod_idx].contains_key(func_id) {
                self.func_map = self.module_func_maps[*mod_idx].clone();
                self.current_mod_idx = *mod_idx;
                let func = self.compile_closure(params, body, captures, mutable_captures);
                code_section.function(&func);
            }
        }

        // _start: call __init_strings, then execute module init code
        {
            // Collect locals PER-MODULE so LocalIds don't collide across modules.
            // Each module declares Lets starting from id 0, so without per-module maps
            // module B's `let id=1` would alias module A's `let id=1`.
            let mut per_module_init_locals: Vec<BTreeMap<LocalId, u32>> =
                Vec::with_capacity(modules.len());
            let mut total_count = 0u32;
            for (_, module) in modules {
                let mut mod_map = BTreeMap::new();
                collect_locals(&module.init, &mut mod_map, &mut total_count, 0);
                per_module_init_locals.push(mod_map);
            }
            // Empty fallback map for global initializers and class field inits that
            // shouldn't reference module-level lets.
            let init_locals: BTreeMap<LocalId, u32> = BTreeMap::new();

            let num_locals = total_count;
            let start_temp_local = num_locals;
            let start_temp_i32 = num_locals + 3;
            let locals = vec![(num_locals + 3, ValType::I64), (1, ValType::I32)];
            let mut func = Function::new(locals);

            // Call __init_strings first
            func.instruction(&Instruction::Call(init_strings_idx));

            // Initialize globals — swap in per-module func_map for correct FuncRef resolution
            for (mod_idx, (_, module)) in modules.iter().enumerate() {
                self.func_map = self.module_func_maps[mod_idx].clone();
                // Per-consumer import resolution (imported_var_globals /
                // imported_func_indices / imported_ns_funcs) is keyed by
                // current_mod_idx; a module-scope initializer that calls an
                // imported symbol (e.g. `const P = vec3(...)`) resolves
                // against a stale consumer without this and could bind
                // another module's like-named export.
                self.current_mod_idx = mod_idx;
                for global in &module.globals {
                    if let Some(init) = &global.init {
                        let mut ctx =
                            FuncEmitCtx::new(self, &init_locals, start_temp_local, start_temp_i32);
                        ctx.emit_expr(&mut func, init);
                        let gidx = self.global_map[&global.id];
                        func.instruction(&Instruction::GlobalSet(gidx));
                    } else if global.name == "__platform__" {
                        // Web platform ID = 5
                        func.instruction(&f64_const(5.0));
                        func.instruction(&Instruction::I64ReinterpretF64);
                        let gidx = self.global_map[&global.id];
                        func.instruction(&Instruction::GlobalSet(gidx));
                    }
                }
            }

            // Register class methods with the bridge and set up inheritance
            for (mod_idx, (_, module)) in modules.iter().enumerate() {
                self.func_map = self.module_func_maps[mod_idx].clone();
                self.current_mod_idx = mod_idx; // see the globals loop above
                for class in &module.classes {
                    let class_name_id = self
                        .string_map
                        .get(class.name.as_str())
                        .copied()
                        .unwrap_or(0);
                    let class_bits = (STRING_TAG << 48) | (class_name_id as u64);

                    // Register instance methods in classMethodTable (including getters/setters)
                    if let Some(methods) = self.class_method_map.get(&class.name) {
                        for (method_name, &func_idx) in methods {
                            let real_name = method_name.as_str();
                            let method_name_id =
                                self.string_map.get(real_name).copied().unwrap_or(0);
                            let method_bits = (STRING_TAG << 48) | (method_name_id as u64);
                            let table_idx = self
                                .func_to_table_idx
                                .get(&func_idx)
                                .copied()
                                .unwrap_or(func_idx);
                            // Store args to memory for mem_call (Firefox NaN-safe: use I64Store)
                            func.instruction(&Instruction::I32Const(0xFF00));
                            func.instruction(&Instruction::I64Const(class_bits as i64));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                                offset: 0,
                                align: 3,
                                memory_index: 0,
                            }));
                            func.instruction(&Instruction::I32Const(0xFF08));
                            func.instruction(&Instruction::I64Const(method_bits as i64));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                                offset: 0,
                                align: 3,
                                memory_index: 0,
                            }));
                            func.instruction(&Instruction::I32Const(0xFF10));
                            func.instruction(&Instruction::I64Const(
                                (table_idx as f64).to_bits() as i64
                            ));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                                offset: 0,
                                align: 3,
                                memory_index: 0,
                            }));
                            let csm_id = self
                                .string_map
                                .get("class_set_method")
                                .copied()
                                .unwrap_or(0);
                            func.instruction(&f64_const(csm_id as f64));
                            func.instruction(&f64_const(3.0));
                            func.instruction(&Instruction::I32Const(0xFF00));
                            func.instruction(&Instruction::Call(rt.mem_call));
                            func.instruction(&Instruction::Drop);
                        }
                    }

                    // Set up inheritance
                    if let Some(parent_name) = &class.extends_name {
                        let parent_name_id = self
                            .string_map
                            .get(parent_name.as_str())
                            .copied()
                            .unwrap_or(0);
                        let parent_bits = (STRING_TAG << 48) | (parent_name_id as u64);
                        func.instruction(&Instruction::I32Const(0xFF00));
                        func.instruction(&Instruction::I64Const(class_bits as i64));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        func.instruction(&Instruction::I32Const(0xFF08));
                        func.instruction(&Instruction::I64Const(parent_bits as i64));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        let csp_id = self
                            .string_map
                            .get("class_set_parent")
                            .copied()
                            .unwrap_or(0);
                        func.instruction(&f64_const(csp_id as f64));
                        func.instruction(&f64_const(2.0));
                        func.instruction(&Instruction::I32Const(0xFF00));
                        func.instruction(&Instruction::Call(rt.mem_call));
                        func.instruction(&Instruction::Drop);
                    }

                    // Register static fields
                    for field in &class.static_fields {
                        if let Some(init) = &field.init {
                            let field_name_id = self
                                .string_map
                                .get(field.name.as_str())
                                .copied()
                                .unwrap_or(0);
                            let field_bits = (STRING_TAG << 48) | (field_name_id as u64);
                            // Store class name
                            func.instruction(&Instruction::I32Const(0xFF00));
                            func.instruction(&Instruction::I64Const(class_bits as i64));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                                offset: 0,
                                align: 3,
                                memory_index: 0,
                            }));
                            // Store field name
                            func.instruction(&Instruction::I32Const(0xFF08));
                            func.instruction(&Instruction::I64Const(field_bits as i64));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                                offset: 0,
                                align: 3,
                                memory_index: 0,
                            }));
                            // Store value
                            let mut ctx = FuncEmitCtx::new(
                                self,
                                &init_locals,
                                start_temp_local,
                                start_temp_i32,
                            );
                            ctx.emit_expr(&mut func, init);
                            // Use temp local to store the value
                            func.instruction(&Instruction::LocalSet(start_temp_local));
                            func.instruction(&Instruction::I32Const(0xFF10));
                            func.instruction(&Instruction::LocalGet(start_temp_local));
                            // Value is already i64, no conversion needed
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                                offset: 0,
                                align: 3,
                                memory_index: 0,
                            }));
                            let css_id = self
                                .string_map
                                .get("class_set_static")
                                .copied()
                                .unwrap_or(0);
                            func.instruction(&f64_const(css_id as f64));
                            func.instruction(&f64_const(3.0));
                            func.instruction(&Instruction::I32Const(0xFF00));
                            func.instruction(&Instruction::Call(rt.mem_call));
                            func.instruction(&Instruction::Drop);
                        }
                    }
                }
            }

            // Execute init statements from all modules — swap in per-module func_map
            // and per-module local map so LocalGets resolve to the correct WASM local
            // (or fall back to module_let_globals via current_mod_idx).
            for (mod_idx, (_, module)) in modules.iter().enumerate() {
                self.func_map = self.module_func_maps[mod_idx].clone();
                self.current_mod_idx = mod_idx;
                let mod_locals = &per_module_init_locals[mod_idx];
                let mut ctx = FuncEmitCtx::new(self, mod_locals, start_temp_local, start_temp_i32);
                for stmt in &module.init {
                    ctx.emit_stmt(&mut func, stmt, false);
                }
            }

            func.instruction(&Instruction::End);
            code_section.function(&func);
        }

        wasm_module.section(&code_section);

        // --- Data section (string literal bytes, must come after Code) ---
        if !self.string_data.is_empty() {
            let mut data_section = DataSection::new();
            data_section.active(
                0,
                &wasm_encoder::ConstExpr::i32_const(0),
                self.string_data.iter().copied(),
            );
            wasm_module.section(&data_section);
        }

        let wasm_bytes = wasm_module.finish();
        let async_js = self.async_js_code.join("\n");
        let ffi_import_names = self
            .ffi_imports
            .iter()
            .map(|(name, _, _)| name.clone())
            .collect();
        WasmCompileOutput {
            wasm_bytes,
            async_js,
            ffi_imports: ffi_import_names,
        }
    }
}
