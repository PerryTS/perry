//! String-literal collection extracted from emit/mod.rs (#1102 mechanical split).
//!
//! Pure move of `WasmModuleEmitter::{collect_strings, collect_strings_in_stmts,
//! collect_strings_in_stmt, collect_strings_in_expr}` onto a dedicated
//! inherent `impl WasmModuleEmitter` block.

use super::*;

impl WasmModuleEmitter {
    pub(super) fn collect_strings(&mut self, module: &perry_hir::ir::Module) {
        // Pre-intern common strings used by bridge calls
        self.intern_string("Authorization");
        self.intern_string("POST");
        self.intern_string("GET");
        self.intern_string("");

        // Pre-intern ALL bridge function names for mem_call dispatch
        let bridge_names = [
            "console_log",
            "console_warn",
            "console_error",
            "string_concat",
            "js_add",
            "string_eq",
            "string_len",
            "jsvalue_to_string",
            "is_truthy",
            "js_strict_eq",
            "math_floor",
            "math_ceil",
            "math_round",
            "math_abs",
            "math_sqrt",
            "math_pow",
            "math_random",
            "math_log",
            "date_now",
            "js_typeof",
            "math_min",
            "math_max",
            "parse_int",
            "parse_float",
            "js_mod",
            "is_null_or_undefined",
            "object_new",
            "object_set",
            "object_get",
            "object_get_dynamic",
            "object_set_dynamic",
            "object_delete",
            "object_delete_dynamic",
            "object_keys",
            "object_values",
            "object_entries",
            "object_has_property",
            "object_assign",
            "array_new",
            "array_push",
            "array_pop",
            "array_get",
            "array_set",
            "array_length",
            "array_slice",
            "array_splice",
            "array_shift",
            "array_unshift",
            "array_join",
            "array_index_of",
            "array_includes",
            "array_concat",
            "array_reverse",
            "array_flat",
            "array_is_array",
            "array_from",
            "array_push_spread",
            "string_charAt",
            "string_substring",
            "string_indexOf",
            "string_slice",
            "string_toLowerCase",
            "string_toUpperCase",
            "string_trim",
            "string_includes",
            "string_startsWith",
            "string_endsWith",
            "string_replace",
            "string_split",
            "string_fromCharCode",
            "string_padStart",
            "string_padEnd",
            "string_repeat",
            "string_match",
            "math_log2",
            "math_log10",
            // Issue #133 item 4: trig / exp / sign / trunc / cbrt / hypot etc.
            "math_sin",
            "math_cos",
            "math_tan",
            "math_asin",
            "math_acos",
            "math_atan",
            "math_atan2",
            "math_sinh",
            "math_cosh",
            "math_tanh",
            "math_asinh",
            "math_acosh",
            "math_atanh",
            "math_exp",
            "math_expm1",
            "math_log1p",
            "math_sign",
            "math_trunc",
            "math_cbrt",
            "math_hypot",
            "math_fround",
            "math_clz32",
            "math_imul",
            "closure_new",
            "closure_set_capture",
            "closure_call_0",
            "closure_call_1",
            "closure_call_2",
            "closure_call_3",
            "closure_call_spread",
            "array_map",
            "array_filter",
            "array_forEach",
            "array_reduce",
            "array_find",
            "array_find_index",
            "array_sort",
            "array_some",
            "array_every",
            "class_new",
            "class_set_method",
            "class_call_method",
            "class_get_field",
            "class_set_field",
            "class_set_static",
            "class_get_static",
            "class_instanceof",
            "json_parse",
            "json_stringify",
            "map_new",
            "map_set",
            "map_get",
            "map_has",
            "map_delete",
            "map_size",
            "map_clear",
            "map_entries",
            "map_keys",
            "map_values",
            "set_new",
            "set_new_from_array",
            "set_add",
            "set_has",
            "set_delete",
            "set_size",
            "set_clear",
            "set_values",
            "date_new_val",
            "date_get_time",
            "date_to_iso_string",
            "date_get_full_year",
            "date_get_month",
            "date_get_date",
            "date_get_day",
            "date_get_hours",
            "date_get_minutes",
            "date_get_seconds",
            "date_get_milliseconds",
            "error_new",
            "error_message",
            "regexp_new",
            "regexp_test",
            "number_coerce",
            "is_nan",
            "is_finite",
            "console_log_multi",
            "class_set_parent",
            "try_start",
            "try_end",
            "throw_value",
            "has_exception",
            "get_exception",
            "url_parse",
            "url_get_href",
            "url_get_pathname",
            "url_get_hostname",
            "url_get_port",
            "url_get_search",
            "url_get_hash",
            "url_get_origin",
            "url_get_protocol",
            "url_get_search_params",
            "searchparams_get",
            "searchparams_has",
            "searchparams_set",
            "searchparams_append",
            "searchparams_delete",
            "searchparams_to_string",
            "crypto_random_uuid",
            "crypto_random_bytes",
            "path_join",
            "path_dirname",
            "path_basename",
            "path_extname",
            "path_resolve",
            "path_is_absolute",
            "os_platform",
            "process_argv",
            "process_cwd",
            "buffer_alloc",
            "buffer_from_string",
            "buffer_to_string",
            "buffer_get",
            "buffer_set",
            "buffer_length",
            "buffer_slice",
            "buffer_concat",
            "uint8array_new",
            "uint8array_from",
            "uint8array_length",
            "uint8array_get",
            "uint8array_set",
            "set_timeout",
            "set_interval",
            "clear_timeout",
            "clear_interval",
            "response_status",
            "response_ok",
            "response_headers_get",
            "response_url",
            "buffer_copy",
            "buffer_write",
            "buffer_equals",
            "buffer_is_buffer",
            "buffer_byte_length",
            "crypto_sha256",
            "crypto_md5",
            "fetch_url",
            "fetch_with_options",
            "response_json",
            "response_text",
            "promise_new",
            "promise_resolve",
            "promise_then",
            "await_promise",
        ];
        for name in &bridge_names {
            self.intern_string(name);
        }

        for func in &module.functions {
            self.collect_strings_in_stmts(&func.body);
        }
        for stmt in &module.init {
            self.collect_strings_in_stmt(stmt);
        }
        for global in &module.globals {
            if let Some(init) = &global.init {
                self.collect_strings_in_expr(init);
            }
        }
        // Collect enum names and member names/values
        for enum_def in &module.enums {
            self.intern_string(&enum_def.name);
            for member in &enum_def.members {
                self.intern_string(&member.name);
                match &member.value {
                    EnumValue::String(s) => {
                        self.intern_string(s);
                        self.enum_values.insert(
                            (enum_def.name.clone(), member.name.clone()),
                            EnumResolvedValue::String(s.clone()),
                        );
                    }
                    EnumValue::Number(n) => {
                        self.enum_values.insert(
                            (enum_def.name.clone(), member.name.clone()),
                            EnumResolvedValue::Number(*n as f64),
                        );
                    }
                }
            }
        }
        // Collect class names and method/field names
        for class in &module.classes {
            self.intern_string(&class.name);
            if let Some(parent) = &class.extends_name {
                self.intern_string(parent);
            }
            if let Some(ctor) = &class.constructor {
                self.collect_strings_in_stmts(&ctor.body);
                for param in &ctor.params {
                    if let Some(default) = &param.default {
                        self.collect_strings_in_expr(default);
                    }
                }
            }
            for method in &class.methods {
                self.intern_string(&method.name);
                self.collect_strings_in_stmts(&method.body);
            }
            for method in &class.static_methods {
                self.intern_string(&method.name);
                self.collect_strings_in_stmts(&method.body);
            }
            for (name, getter) in &class.getters {
                self.intern_string(name);
                self.intern_string(&format!("__get_{}", name));
                self.collect_strings_in_stmts(&getter.body);
            }
            for (name, setter) in &class.setters {
                self.intern_string(name);
                self.intern_string(&format!("__set_{}", name));
                self.collect_strings_in_stmts(&setter.body);
            }
            for field in &class.fields {
                self.intern_string(&field.name);
                if let Some(init) = &field.init {
                    self.collect_strings_in_expr(init);
                }
            }
            for field in &class.static_fields {
                self.intern_string(&field.name);
                if let Some(init) = &field.init {
                    self.collect_strings_in_expr(init);
                }
            }
        }
    }

    pub(super) fn collect_strings_in_stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.collect_strings_in_stmt(stmt);
        }
    }

    pub(super) fn collect_strings_in_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.collect_strings_in_expr(e);
                }
            }
            Stmt::Expr(e) => self.collect_strings_in_expr(e),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.collect_strings_in_expr(e);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.collect_strings_in_expr(condition);
                self.collect_strings_in_stmts(then_branch);
                if let Some(eb) = else_branch {
                    self.collect_strings_in_stmts(eb);
                }
            }
            Stmt::While { condition, body } => {
                self.collect_strings_in_expr(condition);
                self.collect_strings_in_stmts(body);
            }
            Stmt::DoWhile { body, condition } => {
                self.collect_strings_in_stmts(body);
                self.collect_strings_in_expr(condition);
            }
            Stmt::Labeled { body, .. } => {
                self.collect_strings_in_stmt(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.collect_strings_in_stmt(i);
                }
                if let Some(c) = condition {
                    self.collect_strings_in_expr(c);
                }
                if let Some(u) = update {
                    self.collect_strings_in_expr(u);
                }
                self.collect_strings_in_stmts(body);
            }
            Stmt::Throw(e) => self.collect_strings_in_expr(e),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.collect_strings_in_stmts(body);
                if let Some(c) = catch {
                    self.collect_strings_in_stmts(&c.body);
                }
                if let Some(f) = finally {
                    self.collect_strings_in_stmts(f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.collect_strings_in_expr(discriminant);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.collect_strings_in_expr(t);
                    }
                    self.collect_strings_in_stmts(&case.body);
                }
            }
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }

    pub(super) fn collect_strings_in_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::String(s) => {
                self.intern_string(s);
            }
            Expr::Binary { left, right, .. }
            | Expr::Compare { left, right, .. }
            | Expr::Logical { left, right, .. } => {
                self.collect_strings_in_expr(left);
                self.collect_strings_in_expr(right);
            }
            Expr::Unary { operand, .. } => self.collect_strings_in_expr(operand),
            Expr::Call { callee, args, .. } => {
                self.collect_strings_in_expr(callee);
                for a in args {
                    self.collect_strings_in_expr(a);
                }
            }
            Expr::LocalSet(_, val) | Expr::GlobalSet(_, val) => {
                self.collect_strings_in_expr(val);
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.collect_strings_in_expr(condition);
                self.collect_strings_in_expr(then_expr);
                self.collect_strings_in_expr(else_expr);
            }
            Expr::Closure { body, .. } => {
                self.collect_strings_in_stmts(body);
            }
            Expr::NativeMethodCall {
                module,
                method,
                args,
                class_name,
                object,
            } => {
                for a in args {
                    self.collect_strings_in_expr(a);
                }
                if let Some(obj) = object {
                    self.collect_strings_in_expr(obj);
                }
                // Pre-intern bridge name for UI calls
                let normalized = module.strip_prefix("node:").unwrap_or(module);
                if normalized == "perry/ui" || normalized == "perry/system" {
                    let bridge_name = map_ui_method(method, class_name.as_deref());
                    self.intern_string(bridge_name);
                }
                if normalized == "perry/thread" {
                    match method.as_str() {
                        "parallelMap" => {
                            self.intern_string("thread_parallel_map");
                        }
                        "parallelFilter" => {
                            self.intern_string("thread_parallel_filter");
                        }
                        "spawn" => {
                            self.intern_string("thread_spawn");
                        }
                        _ => {}
                    }
                }
            }
            Expr::Array(elems) => {
                for e in elems {
                    self.collect_strings_in_expr(e);
                }
            }
            Expr::Object(fields) => {
                for (k, v) in fields {
                    self.intern_string(k);
                    self.collect_strings_in_expr(v);
                }
            }
            Expr::PropertyGet { object, property } => {
                self.collect_strings_in_expr(object);
                self.intern_string(property);
            }
            Expr::PropertySet {
                object,
                value,
                property,
                ..
            } => {
                self.collect_strings_in_expr(object);
                self.collect_strings_in_expr(value);
                self.intern_string(property);
            }
            Expr::IndexGet { object, index } => {
                self.collect_strings_in_expr(object);
                self.collect_strings_in_expr(index);
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                self.collect_strings_in_expr(object);
                self.collect_strings_in_expr(index);
                self.collect_strings_in_expr(value);
            }
            // #5016: `obj.prop = v` / `obj[i] = v` lower to PutValueSet. Recurse
            // into all operands so the key string (when an `Expr::String`) and
            // any string literals in the value/target are interned — the emit
            // arm reads the key via `string_map`.
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } => {
                self.collect_strings_in_expr(target);
                self.collect_strings_in_expr(key);
                self.collect_strings_in_expr(value);
                self.collect_strings_in_expr(receiver);
            }
            Expr::Await(e) | Expr::TypeOf(e) | Expr::Void(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::New { args, .. } => {
                for a in args {
                    self.collect_strings_in_expr(a);
                }
            }
            Expr::Update { .. } => {}
            Expr::Sequence(exprs) => {
                for e in exprs {
                    self.collect_strings_in_expr(e);
                }
            }
            Expr::EnumMember {
                enum_name,
                member_name,
            } => {
                self.intern_string(enum_name);
                self.intern_string(member_name);
            }
            Expr::StaticFieldGet {
                class_name,
                field_name,
            }
            | Expr::StaticFieldSet {
                class_name,
                field_name,
                ..
            } => {
                self.intern_string(class_name);
                self.intern_string(field_name);
            }
            Expr::StaticMethodCall {
                class_name,
                method_name,
                args,
            } => {
                self.intern_string(class_name);
                self.intern_string(method_name);
                for a in args {
                    self.collect_strings_in_expr(a);
                }
            }
            Expr::InstanceOf { expr, ty, .. } => {
                self.collect_strings_in_expr(expr);
                self.intern_string(ty);
            }
            Expr::In { property, object } => {
                self.collect_strings_in_expr(property);
                self.collect_strings_in_expr(object);
            }
            Expr::Delete(e) => self.collect_strings_in_expr(e),
            Expr::RegExp { pattern, flags } => {
                self.intern_string(pattern);
                self.intern_string(flags);
            }
            Expr::RegExpTest { regex, string } => {
                self.collect_strings_in_expr(regex);
                self.collect_strings_in_expr(string);
            }
            Expr::StringMatch { string, regex } => {
                self.collect_strings_in_expr(string);
                self.collect_strings_in_expr(regex);
            }
            Expr::StringReplace {
                string,
                pattern,
                replacement,
            } => {
                self.collect_strings_in_expr(string);
                self.collect_strings_in_expr(pattern);
                self.collect_strings_in_expr(replacement);
            }
            Expr::StringSplit(a, b) => {
                self.collect_strings_in_expr(a);
                self.collect_strings_in_expr(b);
            }
            Expr::StringFromCharCode(e) | Expr::StringFromCodePoint(e) | Expr::StringCoerce(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::StringAt { string, index } | Expr::StringCodePointAt { string, index } => {
                self.collect_strings_in_expr(string);
                self.collect_strings_in_expr(index);
            }
            Expr::ObjectSpread { parts } => {
                for (key_opt, val) in parts {
                    if let Some(k) = key_opt {
                        self.intern_string(k);
                    }
                    self.collect_strings_in_expr(val);
                }
            }
            Expr::ArraySpread(elements) => {
                for elem in elements {
                    match elem {
                        ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                            self.collect_strings_in_expr(e);
                        }
                        ArrayElement::Hole => {}
                    }
                }
            }
            Expr::ObjectKeys(e) | Expr::ObjectValues(e) | Expr::ObjectEntries(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::ObjectRest {
                object,
                exclude_keys,
            } => {
                self.collect_strings_in_expr(object);
                for k in exclude_keys {
                    self.intern_string(k);
                }
            }
            Expr::ArrayPush { value, .. } | Expr::ArrayUnshift { value, .. } => {
                self.collect_strings_in_expr(value);
            }
            Expr::ArrayPushSpread { source, .. } => {
                self.collect_strings_in_expr(source);
            }
            Expr::ArraySlice { array, start, end } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(start);
                if let Some(e) = end {
                    self.collect_strings_in_expr(e);
                }
            }
            Expr::ArraySplice {
                start,
                delete_count,
                items,
                ..
            } => {
                self.collect_strings_in_expr(start);
                if let Some(dc) = delete_count {
                    self.collect_strings_in_expr(dc);
                }
                for item in items {
                    self.collect_strings_in_expr(item);
                }
            }
            Expr::ArrayJoin { array, separator } => {
                self.collect_strings_in_expr(array);
                if let Some(s) = separator {
                    self.collect_strings_in_expr(s);
                }
                self.intern_string(","); // default separator
            }
            Expr::ArrayIndexOf {
                array,
                value,
                from_index,
            }
            | Expr::ArrayIncludes {
                array,
                value,
                from_index,
            } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(value);
                if let Some(fi) = from_index {
                    self.collect_strings_in_expr(fi);
                }
            }
            Expr::ArrayMap { array, callback }
            | Expr::ArrayFilter { array, callback }
            | Expr::ArrayForEach { array, callback }
            | Expr::ArrayFind { array, callback }
            | Expr::ArrayFindIndex { array, callback }
            | Expr::ArraySort {
                array,
                comparator: callback,
            } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(callback);
            }
            Expr::ArrayReduce {
                array,
                callback,
                initial,
            }
            | Expr::ArrayReduceRight {
                array,
                callback,
                initial,
            } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(callback);
                if let Some(i) = initial {
                    self.collect_strings_in_expr(i);
                }
            }
            Expr::ArrayToSorted { array, comparator } => {
                self.collect_strings_in_expr(array);
                if let Some(c) = comparator {
                    self.collect_strings_in_expr(c);
                }
            }
            Expr::ArrayToSpliced {
                array,
                start,
                delete_count,
                items,
            } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(start);
                self.collect_strings_in_expr(delete_count);
                for item in items {
                    self.collect_strings_in_expr(item);
                }
            }
            Expr::ArrayWith {
                array,
                index,
                value,
            } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(index);
                self.collect_strings_in_expr(value);
            }
            Expr::ArrayCopyWithin {
                target, start, end, ..
            } => {
                self.collect_strings_in_expr(target);
                self.collect_strings_in_expr(start);
                if let Some(e) = end {
                    self.collect_strings_in_expr(e);
                }
            }
            Expr::ArrayFlat { array }
            | Expr::ArrayIsArray(array)
            | Expr::ArrayFrom(array)
            | Expr::ArrayToReversed { array } => {
                self.collect_strings_in_expr(array);
            }
            Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
                self.collect_strings_in_expr(array);
            }
            Expr::ArrayFromMapped {
                iterable,
                map_fn,
                this_arg,
            } => {
                self.collect_strings_in_expr(iterable);
                self.collect_strings_in_expr(map_fn);
                if let Some(t) = this_arg {
                    self.collect_strings_in_expr(t);
                }
            }
            Expr::MapSet { map, key, value } => {
                self.collect_strings_in_expr(map);
                self.collect_strings_in_expr(key);
                self.collect_strings_in_expr(value);
            }
            Expr::MapGet { map, key }
            | Expr::MapHas { map, key }
            | Expr::MapDelete { map, key } => {
                self.collect_strings_in_expr(map);
                self.collect_strings_in_expr(key);
            }
            Expr::MapSize(e)
            | Expr::MapClear(e)
            | Expr::MapEntries(e)
            | Expr::MapKeys(e)
            | Expr::MapValues(e)
            | Expr::MapNewFromArray(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::SetNewFromArray(e)
            | Expr::SetSize(e)
            | Expr::SetClear(e)
            | Expr::SetValues(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::SetAdd { value, .. } => {
                self.collect_strings_in_expr(value);
            }
            Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
                self.collect_strings_in_expr(set);
                self.collect_strings_in_expr(value);
            }
            Expr::DateNew(args) => {
                for a in args {
                    self.collect_strings_in_expr(a);
                }
            }
            Expr::DateGetTime(e)
            | Expr::DateToISOString(e)
            | Expr::DateGetFullYear(e)
            | Expr::DateGetMonth(e)
            | Expr::DateGetDate(e)
            | Expr::DateGetDay(e)
            | Expr::DateGetHours(e)
            | Expr::DateGetMinutes(e)
            | Expr::DateGetSeconds(e)
            | Expr::DateGetMilliseconds(e)
            | Expr::DateGetUtcDay(e)
            | Expr::DateGetUtcFullYear(e)
            | Expr::DateGetUtcMonth(e)
            | Expr::DateGetUtcDate(e)
            | Expr::DateGetUtcHours(e)
            | Expr::DateGetUtcMinutes(e)
            | Expr::DateGetUtcSeconds(e)
            | Expr::DateGetUtcMilliseconds(e)
            | Expr::DateValueOf(e)
            | Expr::DateToDateString(e)
            | Expr::DateToTimeString(e)
            | Expr::DateToUTCString(e)
            | Expr::DateToLocaleDateString(e)
            | Expr::DateToLocaleTimeString(e)
            | Expr::DateToLocaleString(e)
            | Expr::DateGetTimezoneOffset(e)
            | Expr::DateToJSON(e)
            | Expr::DateParse(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::DateUtc(args) => {
                for a in args {
                    self.collect_strings_in_expr(a);
                }
            }
            Expr::DateSetUtcFullYear { date, args }
            | Expr::DateSetUtcMonth { date, args }
            | Expr::DateSetUtcDate { date, args }
            | Expr::DateSetUtcHours { date, args }
            | Expr::DateSetUtcMinutes { date, args }
            | Expr::DateSetUtcSeconds { date, args }
            | Expr::DateSetUtcMilliseconds { date, args }
            | Expr::DateSetFullYear { date, args }
            | Expr::DateSetMonth { date, args }
            | Expr::DateSetDate { date, args }
            | Expr::DateSetHours { date, args }
            | Expr::DateSetMinutes { date, args }
            | Expr::DateSetSeconds { date, args }
            | Expr::DateSetMilliseconds { date, args }
            | Expr::DateSetTime { date, args } => {
                self.collect_strings_in_expr(date);
                for a in args {
                    self.collect_strings_in_expr(a);
                }
            }
            Expr::ErrorNew(msg) => {
                if let Some(m) = msg {
                    self.collect_strings_in_expr(m);
                }
            }
            Expr::ErrorMessage(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::ErrorNewWithCause { message, cause } => {
                self.collect_strings_in_expr(message);
                self.collect_strings_in_expr(cause);
            }
            Expr::ErrorNewWithOptions {
                message, options, ..
            } => {
                self.collect_strings_in_expr(message);
                self.collect_strings_in_expr(options);
            }
            Expr::TypeErrorNew(m)
            | Expr::RangeErrorNew(m)
            | Expr::ReferenceErrorNew(m)
            | Expr::SyntaxErrorNew(m) => {
                self.collect_strings_in_expr(m);
            }
            Expr::AggregateErrorNew {
                errors,
                message,
                options,
            } => {
                self.collect_strings_in_expr(errors);
                self.collect_strings_in_expr(message);
                if let Some(o) = options {
                    self.collect_strings_in_expr(o);
                }
            }
            Expr::JsonParse(e) | Expr::JsonStringify(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::NumberCoerce(e)
            | Expr::IsNaN(e)
            | Expr::IsUndefinedOrBareNan(e)
            | Expr::IsFinite(e)
            | Expr::BigIntCoerce(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::ParseInt { string, radix } => {
                self.collect_strings_in_expr(string);
                if let Some(r) = radix {
                    self.collect_strings_in_expr(r);
                }
            }
            Expr::ParseFloat(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::PropertyUpdate {
                object, property, ..
            } => {
                self.collect_strings_in_expr(object);
                self.intern_string(property);
            }
            Expr::IndexUpdate { object, index, .. } => {
                self.collect_strings_in_expr(object);
                self.collect_strings_in_expr(index);
            }
            Expr::SuperCall(args) => {
                for a in args {
                    self.collect_strings_in_expr(a);
                }
            }
            Expr::SuperMethodCall { method, args } => {
                self.intern_string(method);
                for a in args {
                    self.collect_strings_in_expr(a);
                }
            }
            Expr::NewDynamic { callee, args, .. } => {
                self.collect_strings_in_expr(callee);
                for a in args {
                    self.collect_strings_in_expr(a);
                }
            }
            Expr::FetchWithOptions {
                url,
                method,
                body,
                headers,
                headers_dynamic,
                signal,
            } => {
                self.collect_strings_in_expr(url);
                self.collect_strings_in_expr(method);
                self.collect_strings_in_expr(body);
                for (key, val) in headers {
                    self.intern_string(key);
                    self.collect_strings_in_expr(val);
                }
                if let Some(hd) = headers_dynamic {
                    self.collect_strings_in_expr(hd);
                }
                if let Some(s) = signal {
                    self.collect_strings_in_expr(s);
                }
            }
            Expr::FetchGetWithAuth { url, auth_header } => {
                self.collect_strings_in_expr(url);
                self.collect_strings_in_expr(auth_header);
            }
            Expr::FetchPostWithAuth {
                url,
                auth_header,
                body,
            } => {
                self.collect_strings_in_expr(url);
                self.collect_strings_in_expr(auth_header);
                self.collect_strings_in_expr(body);
            }
            _ => {}
        }
    }
}
