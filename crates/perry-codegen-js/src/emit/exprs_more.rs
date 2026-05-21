use super::*;
use std::fmt::Write as FmtWrite;

impl JsEmitter {
    pub fn emit_expr_continued(&mut self, expr: &Expr) {
        match expr {
            // --- Child process (throw stubs) ---
            Expr::ChildProcessExecSync { .. }
            | Expr::ChildProcessSpawnSync { .. }
            | Expr::ChildProcessSpawn { .. }
            | Expr::ChildProcessExec { .. }
            | Expr::ChildProcessSpawnBackground { .. }
            | Expr::ChildProcessGetProcessStatus(_)
            | Expr::ChildProcessKillProcess(_) => {
                self.output.push_str(
                    "((() => { throw new Error('child_process not available in browser'); })())",
                );
            }

            // --- Fetch ---
            Expr::FetchWithOptions {
                url,
                method,
                body,
                headers,
            } => {
                self.output.push_str("fetch(");
                self.emit_expr(url);
                self.output.push_str(", {method: ");
                self.emit_expr(method);
                self.output.push_str(", body: ");
                self.emit_expr(body);
                self.output.push_str(", headers: {");
                for (i, (key, val)) in headers.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.output.push_str(&self.quote_string(key));
                    self.output.push_str(": ");
                    self.emit_expr(val);
                }
                self.output.push_str("}})");
            }
            Expr::FetchGetWithAuth { url, auth_header } => {
                self.output.push_str("fetch(");
                self.emit_expr(url);
                self.output.push_str(", {headers: {\"Authorization\": ");
                self.emit_expr(auth_header);
                self.output.push_str("}})");
            }
            Expr::FetchPostWithAuth {
                url,
                auth_header,
                body,
            } => {
                self.output.push_str("fetch(");
                self.emit_expr(url);
                self.output
                    .push_str(", {method: \"POST\", headers: {\"Authorization\": ");
                self.emit_expr(auth_header);
                self.output
                    .push_str(", \"Content-Type\": \"application/json\"}, body: ");
                self.emit_expr(body);
                self.output.push_str("})");
            }

            // --- Net (throw stubs) ---
            Expr::NetCreateServer { .. }
            | Expr::NetCreateConnection { .. }
            | Expr::NetConnect { .. } => {
                self.output.push_str(
                    "((() => { throw new Error('net module not available in browser'); })())",
                );
            }

            // --- Array methods ---
            Expr::ArrayPush { array_id, value } => {
                let name = self.get_local_name(*array_id);
                let _ = write!(self.output, "{}.push(", name);
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::ArrayPushSpread { array_id, source } => {
                let name = self.get_local_name(*array_id);
                let _ = write!(self.output, "{}.push(...", name);
                self.emit_expr(source);
                self.output.push(')');
            }
            Expr::ArrayPop(id) => {
                let name = self.get_local_name(*id);
                let _ = write!(self.output, "{}.pop()", name);
            }
            Expr::ArrayShift(id) => {
                let name = self.get_local_name(*id);
                let _ = write!(self.output, "{}.shift()", name);
            }
            Expr::ArrayUnshift { array_id, value } => {
                let name = self.get_local_name(*array_id);
                let _ = write!(self.output, "{}.unshift(", name);
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::ArrayIndexOf { array, value } => {
                self.emit_expr(array);
                self.output.push_str(".indexOf(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::ArrayIncludes { array, value } => {
                self.emit_expr(array);
                self.output.push_str(".includes(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::ArraySlice { array, start, end } => {
                self.emit_expr(array);
                self.output.push_str(".slice(");
                self.emit_expr(start);
                if let Some(e) = end {
                    self.output.push_str(", ");
                    self.emit_expr(e);
                }
                self.output.push(')');
            }
            Expr::ArraySplice {
                array_id,
                start,
                delete_count,
                items,
            } => {
                let name = self.get_local_name(*array_id);
                let _ = write!(self.output, "{}.splice(", name);
                self.emit_expr(start);
                if let Some(dc) = delete_count {
                    self.output.push_str(", ");
                    self.emit_expr(dc);
                }
                for item in items {
                    self.output.push_str(", ");
                    self.emit_expr(item);
                }
                self.output.push(')');
            }

            // --- Array higher-order methods ---
            Expr::ArrayForEach { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".forEach(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayMap { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".map(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayFilter { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".filter(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayFind { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".find(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayFindIndex { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".findIndex(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayFindLast { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".findLast(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayFindLastIndex { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".findLastIndex(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayAt { array, index } => {
                self.emit_expr(array);
                self.output.push_str(".at(");
                self.emit_expr(index);
                self.output.push(')');
            }
            Expr::ArraySome { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".some(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayEvery { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".every(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayFlatMap { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".flatMap(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArraySort { array, comparator } => {
                self.emit_expr(array);
                self.output.push_str(".sort(");
                self.emit_expr(comparator);
                self.output.push(')');
            }
            Expr::ArrayReduce {
                array,
                callback,
                initial,
            } => {
                self.emit_expr(array);
                self.output.push_str(".reduce(");
                self.emit_expr(callback);
                if let Some(init) = initial {
                    self.output.push_str(", ");
                    self.emit_expr(init);
                }
                self.output.push(')');
            }
            Expr::ArrayJoin { array, separator } => {
                self.emit_expr(array);
                self.output.push_str(".join(");
                if let Some(sep) = separator {
                    self.emit_expr(sep);
                }
                self.output.push(')');
            }
            Expr::ArrayFlat { array } => {
                self.emit_expr(array);
                self.output.push_str(".flat()");
            }
            Expr::ArrayReduceRight {
                array,
                callback,
                initial,
            } => {
                self.emit_expr(array);
                self.output.push_str(".reduceRight(");
                self.emit_expr(callback);
                if let Some(init) = initial {
                    self.output.push_str(", ");
                    self.emit_expr(init);
                }
                self.output.push(')');
            }
            Expr::ArrayToReversed { array } => {
                self.emit_expr(array);
                self.output.push_str(".toReversed()");
            }
            Expr::ArrayToSorted { array, comparator } => {
                self.emit_expr(array);
                self.output.push_str(".toSorted(");
                if let Some(cmp) = comparator {
                    self.emit_expr(cmp);
                }
                self.output.push(')');
            }
            Expr::ArrayToSpliced {
                array,
                start,
                delete_count,
                items,
            } => {
                self.emit_expr(array);
                self.output.push_str(".toSpliced(");
                self.emit_expr(start);
                self.output.push_str(", ");
                self.emit_expr(delete_count);
                for item in items {
                    self.output.push_str(", ");
                    self.emit_expr(item);
                }
                self.output.push(')');
            }
            Expr::ArrayWith {
                array,
                index,
                value,
            } => {
                self.emit_expr(array);
                self.output.push_str(".with(");
                self.emit_expr(index);
                self.output.push_str(", ");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::ArrayCopyWithin {
                array_id,
                target,
                start,
                end,
            } => {
                let name = self.get_local_name(*array_id);
                let _ = write!(self.output, "{}.copyWithin(", name);
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(start);
                if let Some(e) = end {
                    self.output.push_str(", ");
                    self.emit_expr(e);
                }
                self.output.push(')');
            }
            Expr::ArrayEntries(array) => {
                self.output.push_str("Array.from(");
                self.emit_expr(array);
                self.output.push_str(".entries())");
            }
            Expr::ArrayKeys(array) => {
                self.output.push_str("Array.from(");
                self.emit_expr(array);
                self.output.push_str(".keys())");
            }
            Expr::ArrayValues(array) => {
                self.output.push_str("Array.from(");
                self.emit_expr(array);
                self.output.push_str(".values())");
            }

            // --- String methods ---
            Expr::StringSplit(string, delimiter) => {
                self.emit_expr(string);
                self.output.push_str(".split(");
                self.emit_expr(delimiter);
                self.output.push(')');
            }
            Expr::StringFromCharCode(code) => {
                self.output.push_str("String.fromCharCode(");
                self.emit_expr(code);
                self.output.push(')');
            }
            Expr::StringFromCodePoint(code) => {
                self.output.push_str("String.fromCodePoint(");
                self.emit_expr(code);
                self.output.push(')');
            }
            Expr::StringAt { string, index } => {
                self.emit_expr(string);
                self.output.push_str(".at(");
                self.emit_expr(index);
                self.output.push(')');
            }
            Expr::StringCodePointAt { string, index } => {
                self.emit_expr(string);
                self.output.push_str(".codePointAt(");
                self.emit_expr(index);
                self.output.push(')');
            }

            // --- Map operations ---
            Expr::MapNew => self.output.push_str("new Map()"),
            Expr::MapNewFromArray(expr) => {
                self.output.push_str("new Map(");
                self.emit_expr(expr);
                self.output.push(')');
            }
            Expr::MapSet { map, key, value } => {
                self.emit_expr(map);
                self.output.push_str(".set(");
                self.emit_expr(key);
                self.output.push_str(", ");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::MapGet { map, key } => {
                self.emit_expr(map);
                self.output.push_str(".get(");
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::MapHas { map, key } => {
                self.emit_expr(map);
                self.output.push_str(".has(");
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::MapDelete { map, key } => {
                self.emit_expr(map);
                self.output.push_str(".delete(");
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::MapSize(map) => {
                self.emit_expr(map);
                self.output.push_str(".size");
            }
            Expr::MapClear(map) => {
                self.emit_expr(map);
                self.output.push_str(".clear()");
            }
            Expr::MapEntries(map) => {
                self.output.push_str("Array.from(");
                self.emit_expr(map);
                self.output.push_str(".entries())");
            }
            Expr::MapKeys(map) => {
                self.output.push_str("Array.from(");
                self.emit_expr(map);
                self.output.push_str(".keys())");
            }
            Expr::MapValues(map) => {
                self.output.push_str("Array.from(");
                self.emit_expr(map);
                self.output.push_str(".values())");
            }

            // --- Set operations ---
            Expr::SetNew => self.output.push_str("new Set()"),
            Expr::SetNewFromArray(expr) => {
                self.output.push_str("new Set(");
                self.emit_expr(expr);
                self.output.push(')');
            }
            Expr::SetAdd { set_id, value } => {
                let name = self.get_local_name(*set_id);
                let _ = write!(self.output, "{}.add(", name);
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::SetHas { set, value } => {
                self.emit_expr(set);
                self.output.push_str(".has(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::SetDelete { set, value } => {
                self.emit_expr(set);
                self.output.push_str(".delete(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::SetSize(set) => {
                self.emit_expr(set);
                self.output.push_str(".size");
            }
            Expr::SetClear(set) => {
                self.emit_expr(set);
                self.output.push_str(".clear()");
            }
            Expr::SetValues(set) => {
                self.output.push_str("Array.from(");
                self.emit_expr(set);
                self.output.push_str(".values())");
            }

            // --- Sequence ---
            Expr::Sequence(exprs) => {
                self.output.push('(');
                for (i, e) in exprs.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(e);
                }
                self.output.push(')');
            }

            // --- Date ---
            Expr::DateNow => self.output.push_str("Date.now()"),
            Expr::DateNew(args) => {
                if args.is_empty() {
                    self.output.push_str("new Date()");
                } else {
                    self.output.push_str("new Date(");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(a);
                    }
                    self.output.push(')');
                }
            }
            Expr::DateGetTime(d) => {
                self.emit_expr(d);
                self.output.push_str(".getTime()");
            }
            Expr::DateToISOString(d) => {
                self.emit_expr(d);
                self.output.push_str(".toISOString()");
            }
            Expr::DateGetFullYear(d) => {
                self.emit_expr(d);
                self.output.push_str(".getFullYear()");
            }
            Expr::DateGetMonth(d) => {
                self.emit_expr(d);
                self.output.push_str(".getMonth()");
            }
            Expr::DateGetDate(d) => {
                self.emit_expr(d);
                self.output.push_str(".getDate()");
            }
            Expr::DateGetDay(d) => {
                self.emit_expr(d);
                self.output.push_str(".getDay()");
            }
            Expr::DateGetHours(d) => {
                self.emit_expr(d);
                self.output.push_str(".getHours()");
            }
            Expr::DateGetMinutes(d) => {
                self.emit_expr(d);
                self.output.push_str(".getMinutes()");
            }
            Expr::DateGetSeconds(d) => {
                self.emit_expr(d);
                self.output.push_str(".getSeconds()");
            }
            Expr::DateGetMilliseconds(d) => {
                self.emit_expr(d);
                self.output.push_str(".getMilliseconds()");
            }
            Expr::DateParse(s) => {
                self.output.push_str("Date.parse(");
                self.emit_expr(s);
                self.output.push(')');
            }
            Expr::DateUtc(args) => {
                self.output.push_str("Date.UTC(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(a);
                }
                self.output.push(')');
            }
            Expr::DateGetUtcDay(d) => {
                self.emit_expr(d);
                self.output.push_str(".getUTCDay()");
            }
            Expr::DateGetUtcFullYear(d) => {
                self.emit_expr(d);
                self.output.push_str(".getUTCFullYear()");
            }
            Expr::DateGetUtcMonth(d) => {
                self.emit_expr(d);
                self.output.push_str(".getUTCMonth()");
            }
            Expr::DateGetUtcDate(d) => {
                self.emit_expr(d);
                self.output.push_str(".getUTCDate()");
            }
            Expr::DateGetUtcHours(d) => {
                self.emit_expr(d);
                self.output.push_str(".getUTCHours()");
            }
            Expr::DateGetUtcMinutes(d) => {
                self.emit_expr(d);
                self.output.push_str(".getUTCMinutes()");
            }
            Expr::DateGetUtcSeconds(d) => {
                self.emit_expr(d);
                self.output.push_str(".getUTCSeconds()");
            }
            Expr::DateGetUtcMilliseconds(d) => {
                self.emit_expr(d);
                self.output.push_str(".getUTCMilliseconds()");
            }
            Expr::DateSetUtcFullYear { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setUTCFullYear(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateSetUtcMonth { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setUTCMonth(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateSetUtcDate { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setUTCDate(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateSetUtcHours { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setUTCHours(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateSetUtcMinutes { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setUTCMinutes(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateSetUtcSeconds { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setUTCSeconds(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateSetUtcMilliseconds { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setUTCMilliseconds(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateSetFullYear { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setFullYear(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateSetMonth { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setMonth(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateSetDate { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setDate(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateSetHours { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setHours(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateSetMinutes { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setMinutes(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateSetSeconds { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setSeconds(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateSetMilliseconds { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setMilliseconds(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateSetTime { date, value } => {
                self.emit_expr(date);
                self.output.push_str(".setTime(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::DateValueOf(d) => {
                self.emit_expr(d);
                self.output.push_str(".valueOf()");
            }
            Expr::DateToDateString(d) => {
                self.emit_expr(d);
                self.output.push_str(".toDateString()");
            }
            Expr::DateToTimeString(d) => {
                self.emit_expr(d);
                self.output.push_str(".toTimeString()");
            }
            Expr::DateToLocaleDateString(d) => {
                self.emit_expr(d);
                self.output.push_str(".toLocaleDateString()");
            }
            Expr::DateToLocaleTimeString(d) => {
                self.emit_expr(d);
                self.output.push_str(".toLocaleTimeString()");
            }
            Expr::DateToLocaleString(d) => {
                self.emit_expr(d);
                self.output.push_str(".toLocaleString()");
            }
            Expr::DateGetTimezoneOffset(d) => {
                self.emit_expr(d);
                self.output.push_str(".getTimezoneOffset()");
            }
            Expr::DateToJSON(d) => {
                self.emit_expr(d);
                self.output.push_str(".toJSON()");
            }

            // --- Error ---
            Expr::ErrorNew(msg) => {
                if let Some(m) = msg {
                    self.output.push_str("new Error(");
                    self.emit_expr(m);
                    self.output.push(')');
                } else {
                    self.output.push_str("new Error()");
                }
            }
            Expr::ErrorMessage(err) => {
                self.emit_expr(err);
                self.output.push_str(".message");
            }
            Expr::ErrorNewWithCause { message, cause } => {
                self.output.push_str("new Error(");
                self.emit_expr(message);
                self.output.push_str(", { cause: ");
                self.emit_expr(cause);
                self.output.push_str(" })");
            }
            Expr::TypeErrorNew(msg) => {
                self.output.push_str("new TypeError(");
                self.emit_expr(msg);
                self.output.push(')');
            }
            Expr::RangeErrorNew(msg) => {
                self.output.push_str("new RangeError(");
                self.emit_expr(msg);
                self.output.push(')');
            }
            Expr::ReferenceErrorNew(msg) => {
                self.output.push_str("new ReferenceError(");
                self.emit_expr(msg);
                self.output.push(')');
            }
            Expr::SyntaxErrorNew(msg) => {
                self.output.push_str("new SyntaxError(");
                self.emit_expr(msg);
                self.output.push(')');
            }
            Expr::AggregateErrorNew { errors, message } => {
                self.output.push_str("new AggregateError(");
                self.emit_expr(errors);
                self.output.push_str(", ");
                self.emit_expr(message);
                self.output.push(')');
            }

            // --- URL ---
            Expr::UrlNew { url, base } => {
                self.output.push_str("new URL(");
                self.emit_expr(url);
                if let Some(b) = base {
                    self.output.push_str(", ");
                    self.emit_expr(b);
                }
                self.output.push(')');
            }
            Expr::UrlGetHref(u) => {
                self.emit_expr(u);
                self.output.push_str(".href");
            }
            Expr::UrlGetPathname(u) => {
                self.emit_expr(u);
                self.output.push_str(".pathname");
            }
            Expr::UrlGetProtocol(u) => {
                self.emit_expr(u);
                self.output.push_str(".protocol");
            }
            Expr::UrlGetHost(u) => {
                self.emit_expr(u);
                self.output.push_str(".host");
            }
            Expr::UrlGetHostname(u) => {
                self.emit_expr(u);
                self.output.push_str(".hostname");
            }
            Expr::UrlGetPort(u) => {
                self.emit_expr(u);
                self.output.push_str(".port");
            }
            Expr::UrlGetSearch(u) => {
                self.emit_expr(u);
                self.output.push_str(".search");
            }
            Expr::UrlGetHash(u) => {
                self.emit_expr(u);
                self.output.push_str(".hash");
            }
            Expr::UrlGetOrigin(u) => {
                self.emit_expr(u);
                self.output.push_str(".origin");
            }
            Expr::UrlGetSearchParams(u) => {
                self.emit_expr(u);
                self.output.push_str(".searchParams");
            }

            // --- URLSearchParams ---
            Expr::UrlSearchParamsNew(init) => {
                if let Some(i) = init {
                    self.output.push_str("new URLSearchParams(");
                    self.emit_expr(i);
                    self.output.push(')');
                } else {
                    self.output.push_str("new URLSearchParams()");
                }
            }
            Expr::UrlSearchParamsGet { params, name } => {
                self.emit_expr(params);
                self.output.push_str(".get(");
                self.emit_expr(name);
                self.output.push(')');
            }
            Expr::UrlSearchParamsHas {
                params,
                name,
                value,
            } => {
                self.emit_expr(params);
                self.output.push_str(".has(");
                self.emit_expr(name);
                if let Some(v) = value {
                    self.output.push_str(", ");
                    self.emit_expr(v);
                }
                self.output.push(')');
            }
            Expr::UrlSearchParamsSet {
                params,
                name,
                value,
            } => {
                self.emit_expr(params);
                self.output.push_str(".set(");
                self.emit_expr(name);
                self.output.push_str(", ");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::UrlSearchParamsAppend {
                params,
                name,
                value,
            } => {
                self.emit_expr(params);
                self.output.push_str(".append(");
                self.emit_expr(name);
                self.output.push_str(", ");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::UrlSearchParamsDelete {
                params,
                name,
                value,
            } => {
                self.emit_expr(params);
                self.output.push_str(".delete(");
                self.emit_expr(name);
                if let Some(v) = value {
                    self.output.push_str(", ");
                    self.emit_expr(v);
                }
                self.output.push(')');
            }
            Expr::UrlSearchParamsToString(params) => {
                self.emit_expr(params);
                self.output.push_str(".toString()");
            }
            Expr::UrlSearchParamsEntries(params) => {
                self.output.push_str("Array.from(");
                self.emit_expr(params);
                self.output.push_str(".entries())");
            }
            Expr::UrlSearchParamsGetAll { params, name } => {
                self.emit_expr(params);
                self.output.push_str(".getAll(");
                self.emit_expr(name);
                self.output.push(')');
            }

            // --- Delete ---
            Expr::Delete(expr) => {
                self.output.push_str("delete ");
                self.emit_expr(expr);
            }

            // --- Closure ---
            Expr::Closure {
                params,
                body,
                is_async,
                ..
            } => {
                if *is_async {
                    self.output.push_str("async ");
                }
                self.output.push('(');
                self.emit_params(params);
                self.output.push_str(") => {\n");
                self.indent += 1;
                for s in body {
                    self.emit_stmt(s);
                }
                self.indent -= 1;
                self.write_indent();
                self.output.push('}');
            }

            // --- RegExp ---
            Expr::RegExp { pattern, flags } => {
                let _ = write!(self.output, "/{}/{}", pattern, flags);
            }
            Expr::RegExpTest { regex, string } => {
                self.emit_expr(regex);
                self.output.push_str(".test(");
                self.emit_expr(string);
                self.output.push(')');
            }
            Expr::StringMatch { string, regex } => {
                self.emit_expr(string);
                self.output.push_str(".match(");
                self.emit_expr(regex);
                self.output.push(')');
            }
            Expr::StringMatchAll { string, regex } => {
                self.emit_expr(string);
                self.output.push_str(".matchAll(");
                self.emit_expr(regex);
                self.output.push(')');
            }
            Expr::StringReplace {
                string,
                pattern,
                replacement,
            } => {
                self.emit_expr(string);
                self.output.push_str(".replace(");
                self.emit_expr(pattern);
                self.output.push_str(", ");
                self.emit_expr(replacement);
                self.output.push(')');
            }

            // --- Object operations ---
            Expr::ObjectKeys(obj) => {
                self.output.push_str("Object.keys(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            Expr::ObjectValues(obj) => {
                self.output.push_str("Object.values(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            Expr::ObjectEntries(obj) => {
                self.output.push_str("Object.entries(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            Expr::ObjectFromEntries(entries) => {
                self.output.push_str("Object.fromEntries(");
                self.emit_expr(entries);
                self.output.push(')');
            }
            Expr::ObjectIs(a, b) => {
                self.output.push_str("Object.is(");
                self.emit_expr(a);
                self.output.push(',');
                self.emit_expr(b);
                self.output.push(')');
            }
            Expr::ObjectHasOwn(obj, key) => {
                self.output.push_str("Object.hasOwn(");
                self.emit_expr(obj);
                self.output.push(',');
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::ObjectRest {
                object,
                exclude_keys,
            } => {
                self.output.push_str("(({");
                for (i, key) in exclude_keys.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.output.push_str(key);
                }
                self.output.push_str("}, ..._rest) => _rest[0])("); // Actually use Object.keys approach
                                                                    // Better approach: use destructuring
                self.output.clear(); // Redo this
                self.output.push_str("((() => { const {");
                for (i, key) in exclude_keys.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.output.push_str(key);
                    self.output.push_str(": _");
                }
                self.output.push_str(", ...__rest} = ");
                self.emit_expr(object);
                self.output.push_str("; return __rest; })())");
            }

            // --- Array static methods ---
            Expr::ArrayIsArray(val) => {
                self.output.push_str("Array.isArray(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::ArrayFrom(val) => {
                self.output.push_str("Array.from(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::IteratorToArray(val) => {
                self.output.push_str("Array.from(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::ArrayFromMapped { iterable, map_fn } => {
                self.output.push_str("Array.from(");
                self.emit_expr(iterable);
                self.output.push_str(", ");
                self.emit_expr(map_fn);
                self.output.push(')');
            }

            // --- Global built-in functions ---
            Expr::ParseInt { string, radix } => {
                self.output.push_str("parseInt(");
                self.emit_expr(string);
                if let Some(r) = radix {
                    self.output.push_str(", ");
                    self.emit_expr(r);
                }
                self.output.push(')');
            }
            Expr::ParseFloat(s) => {
                self.output.push_str("parseFloat(");
                self.emit_expr(s);
                self.output.push(')');
            }
            Expr::NumberCoerce(val) => {
                self.output.push_str("Number(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::BigIntCoerce(val) => {
                self.output.push_str("BigInt(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::StringCoerce(val) => {
                self.output.push_str("String(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::BooleanCoerce(val) => {
                self.output.push_str("Boolean(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::IsNaN(val) => {
                self.output.push_str("isNaN(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::IsUndefinedOrBareNan(val) => {
                // JS fallback: (v === undefined || Number.isNaN(v))
                self.output.push_str("((");
                self.emit_expr(val);
                self.output.push_str(") === undefined || Number.isNaN(");
                self.emit_expr(val);
                self.output.push_str("))");
            }
            Expr::IsFinite(val) => {
                self.output.push_str("isFinite(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::NumberIsNaN(val) => {
                self.output.push_str("Number.isNaN(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::NumberIsFinite(val) => {
                self.output.push_str("Number.isFinite(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::NumberIsInteger(val) => {
                self.output.push_str("Number.isInteger(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::NumberIsSafeInteger(val) => {
                self.output.push_str("Number.isSafeInteger(");
                self.emit_expr(val);
                self.output.push(')');
            }

            // --- Static plugin resolve ---
            Expr::StaticPluginResolve(_) => {
                self.output.push_str("undefined");
            }
            Expr::PerformanceNow => {
                self.output.push_str("performance.now()");
            }
            Expr::TextEncoderNew => {
                self.output.push_str("new TextEncoder()");
            }
            Expr::TextDecoderNew => {
                self.output.push_str("new TextDecoder()");
            }
            Expr::TextEncoderEncode(inner) => {
                self.output.push_str("new TextEncoder().encode(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::TextDecoderDecode(inner) => {
                self.output.push_str("new TextDecoder().decode(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::EncodeURI(inner) => {
                self.output.push_str("encodeURI(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::DecodeURI(inner) => {
                self.output.push_str("decodeURI(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::EncodeURIComponent(inner) => {
                self.output.push_str("encodeURIComponent(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::DecodeURIComponent(inner) => {
                self.output.push_str("decodeURIComponent(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::StructuredClone(inner) => {
                self.output.push_str("structuredClone(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::QueueMicrotask(inner) => {
                self.output.push_str("queueMicrotask(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::Atob(inner) => {
                self.output.push_str("atob(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::Btoa(inner) => {
                self.output.push_str("btoa(");
                self.emit_expr(inner);
                self.output.push(')');
            }

            // --- V8/JS interop (passthrough in browser) ---
            Expr::JsLoadModule { path } => {
                let _ = write!(self.output, "((() => {{ throw new Error('JsLoadModule not supported in browser: {}'); }})())", path);
            }
            Expr::JsGetExport {
                module_handle,
                export_name,
            } => {
                self.emit_expr(module_handle);
                let _ = write!(self.output, ".{}", export_name);
            }
            Expr::JsCallFunction {
                module_handle,
                func_name,
                args,
            } => {
                self.emit_expr(module_handle);
                let _ = write!(self.output, ".{}(", func_name);
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            Expr::JsCallMethod {
                object,
                method_name,
                args,
            } => {
                self.emit_expr(object);
                let _ = write!(self.output, ".{}(", method_name);
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            Expr::JsGetProperty {
                object,
                property_name,
            } => {
                self.emit_expr(object);
                let _ = write!(self.output, ".{}", property_name);
            }
            Expr::JsSetProperty {
                object,
                property_name,
                value,
            } => {
                self.output.push('(');
                self.emit_expr(object);
                let _ = write!(self.output, ".{} = ", property_name);
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::JsNew {
                module_handle,
                class_name,
                args,
            } => {
                self.output.push_str("new ");
                self.emit_expr(module_handle);
                let _ = write!(self.output, ".{}(", class_name);
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            Expr::JsNewFromHandle { constructor, args } => {
                self.output.push_str("new (");
                self.emit_expr(constructor);
                self.output.push_str(")(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            Expr::JsCreateCallback { closure, .. } => {
                self.emit_expr(closure);
            }

            // --- ImportMetaUrl ---
            Expr::ImportMetaUrl(path) => {
                let _ = write!(self.output, "{}", self.quote_string(path));
            }

            // --- Math.imul ---
            Expr::MathImul(a, b) => {
                let _ = write!(self.output, "Math.imul(");
                self.emit_expr(a);
                let _ = write!(self.output, ", ");
                self.emit_expr(b);
                let _ = write!(self.output, ")");
            }
            // #853: `StringFromCodePoint` / `StringAt` / `StringCodePointAt`
            // are already handled in the earlier shared block (around lines
            // 2027–2050). The duplicate arms here were dead — removed.
            // Object property descriptor stubs
            Expr::ObjectDefineProperty(obj, key, desc) => {
                self.output.push_str("Object.defineProperty(");
                self.emit_expr(obj);
                self.output.push_str(", ");
                self.emit_expr(key);
                self.output.push_str(", ");
                self.emit_expr(desc);
                self.output.push(')');
            }
            Expr::ObjectGetOwnPropertyDescriptor(obj, key) => {
                self.output.push_str("Object.getOwnPropertyDescriptor(");
                self.emit_expr(obj);
                self.output.push_str(", ");
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::ObjectGetOwnPropertyNames(obj) => {
                self.output.push_str("Object.getOwnPropertyNames(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            Expr::ObjectCreate(proto) => {
                self.output.push_str("Object.create(");
                self.emit_expr(proto);
                self.output.push(')');
            }
            Expr::ObjectFreeze(obj) => {
                self.output.push_str("Object.freeze(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            Expr::ObjectSeal(obj) => {
                self.output.push_str("Object.seal(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            Expr::ObjectPreventExtensions(obj) => {
                self.output.push_str("Object.preventExtensions(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            Expr::ObjectIsFrozen(obj) => {
                self.output.push_str("Object.isFrozen(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            Expr::ObjectIsSealed(obj) => {
                self.output.push_str("Object.isSealed(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            Expr::ObjectIsExtensible(obj) => {
                self.output.push_str("Object.isExtensible(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            Expr::ObjectGetPrototypeOf(obj) => {
                self.output.push_str("Object.getPrototypeOf(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            Expr::ObjectSetPrototypeOf(obj, proto) => {
                self.output.push_str("Object.setPrototypeOf(");
                self.emit_expr(obj);
                self.output.push(',');
                self.emit_expr(proto);
                self.output.push(')');
            }
            Expr::ObjectDefineProperties(target, descs) => {
                self.output.push_str("Object.defineProperties(");
                self.emit_expr(target);
                self.output.push(',');
                self.emit_expr(descs);
                self.output.push(')');
            }
            Expr::ObjectGetOwnPropertySymbols(obj) => {
                self.output.push_str("Object.getOwnPropertySymbols(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            // Symbol stubs
            Expr::SymbolNew(desc) => {
                self.output.push_str("Symbol(");
                if let Some(d) = desc {
                    self.emit_expr(d);
                }
                self.output.push(')');
            }
            Expr::SymbolFor(key) => {
                self.output.push_str("Symbol.for(");
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::SymbolKeyFor(sym) => {
                self.output.push_str("Symbol.keyFor(");
                self.emit_expr(sym);
                self.output.push(')');
            }
            Expr::SymbolDescription(sym) => {
                self.emit_expr(sym);
                self.output.push_str(".description");
            }
            Expr::SymbolToString(sym) => {
                self.emit_expr(sym);
                self.output.push_str(".toString()");
            }
            // RegExp stubs
            Expr::RegExpExec { regex, string } => {
                self.emit_expr(regex);
                self.output.push_str(".exec(");
                self.emit_expr(string);
                self.output.push(')');
            }
            Expr::RegExpSource(re) => {
                self.emit_expr(re);
                self.output.push_str(".source");
            }
            Expr::RegExpFlags(re) => {
                self.emit_expr(re);
                self.output.push_str(".flags");
            }
            Expr::RegExpLastIndex(re) => {
                self.emit_expr(re);
                self.output.push_str(".lastIndex");
            }
            Expr::RegExpSetLastIndex { regex, value } => {
                self.emit_expr(regex);
                self.output.push_str(".lastIndex = ");
                self.emit_expr(value);
            }
            Expr::RegExpReplaceFn {
                string,
                regex,
                callback,
            } => {
                self.emit_expr(string);
                self.output.push_str(".replace(");
                self.emit_expr(regex);
                self.output.push_str(", ");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::RegExpExecIndex => {
                self.output.push_str("__perry_exec_index");
            }
            Expr::RegExpExecGroups => {
                self.output.push_str("__perry_exec_groups");
            }
            // Proxy / Reflect — JS backend emits direct JS forms.
            Expr::ProxyNew { target, handler } => {
                self.output.push_str("new Proxy(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(handler);
                self.output.push(')');
            }
            Expr::ProxyGet { proxy, key } => {
                self.emit_expr(proxy);
                self.output.push('[');
                self.emit_expr(key);
                self.output.push(']');
            }
            Expr::ProxySet { proxy, key, value } => {
                self.output.push('(');
                self.emit_expr(proxy);
                self.output.push('[');
                self.emit_expr(key);
                self.output.push_str("] = ");
                self.emit_expr(value);
                self.output.push_str(", true)");
            }
            Expr::ProxyHas { proxy, key } => {
                self.output.push('(');
                self.emit_expr(key);
                self.output.push_str(" in ");
                self.emit_expr(proxy);
                self.output.push(')');
            }
            Expr::ProxyDelete { proxy, key } => {
                self.output.push_str("delete ");
                self.emit_expr(proxy);
                self.output.push('[');
                self.emit_expr(key);
                self.output.push(']');
            }
            Expr::ProxyApply { proxy, args } => {
                self.emit_expr(proxy);
                self.output.push('(');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(a);
                }
                self.output.push(')');
            }
            Expr::ProxyConstruct { proxy, args } => {
                self.output.push_str("new ");
                self.emit_expr(proxy);
                self.output.push('(');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(a);
                }
                self.output.push(')');
            }
            Expr::ProxyRevocable { target, handler } => {
                self.output.push_str("Proxy.revocable(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(handler);
                self.output.push(')');
            }
            Expr::ProxyRevoke(_) => {
                self.output.push_str("undefined");
            }
            Expr::ReflectGet { target, key } => {
                self.output.push_str("Reflect.get(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::ReflectSet { target, key, value } => {
                self.output.push_str("Reflect.set(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(key);
                self.output.push_str(", ");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::ReflectHas { target, key } => {
                self.output.push_str("Reflect.has(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::ReflectDelete { target, key } => {
                self.output.push_str("Reflect.deleteProperty(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::ReflectOwnKeys(target) => {
                self.output.push_str("Reflect.ownKeys(");
                self.emit_expr(target);
                self.output.push(')');
            }
            Expr::ReflectApply {
                func,
                this_arg,
                args,
            } => {
                self.output.push_str("Reflect.apply(");
                self.emit_expr(func);
                self.output.push_str(", ");
                self.emit_expr(this_arg);
                self.output.push_str(", ");
                self.emit_expr(args);
                self.output.push(')');
            }
            Expr::ReflectConstruct { target, args } => {
                self.output.push_str("Reflect.construct(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(args);
                self.output.push(')');
            }
            Expr::ReflectDefineProperty {
                target,
                key,
                descriptor,
            } => {
                self.output.push_str("Reflect.defineProperty(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(key);
                self.output.push_str(", ");
                self.emit_expr(descriptor);
                self.output.push(')');
            }
            Expr::ReflectGetPrototypeOf(target) => {
                self.output.push_str("Reflect.getPrototypeOf(");
                self.emit_expr(target);
                self.output.push(')');
            }
            // Fallback for HIR variants the JS emitter doesn't model directly
            // (e.g. TypedArrayNew). Emit `undefined` so the emitted JS still
            // parses; these paths are unused for the LLVM-backend sweeps.
            _ => self.output.push_str("undefined"),
        }
    }
}
