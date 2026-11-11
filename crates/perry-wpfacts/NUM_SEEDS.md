# NUM seed audit (P3 B1, DESIGN-p3 rev 3.1 §2.2, REVIEW-p3 D2)

Every non-test use of `prim::NUM` in perry-wpfacts, one row each. A **seed** puts NUM into a node; its citation names the runtime function or the codegen lowering that produces the value, which must be a plain double (canonical IEEE, never a NaN-boxed tag). A **read/map** row tests the fact or maps a family to its prims and seeds nothing.

There are no non-test callers of `JSValue::int32` in perry-runtime or perry-stdlib (DESIGN-p3 §1.1). So every runtime Number below is built by `JSValue::number` / `as f64` / f64 arithmetic. Raw-bits ingress (v8.deserialize, bun:ffi f64/f32) is not modeled by wpfacts (those values are ⊤) and is fixed separately (lane NANB).

`tests/num_seeds_audit.rs` fails when the per-file count of `prim::NUM` uses differs from this table, so a new seed cannot land without a row.

| file:line | kind | value and producer |
|---|---|---|
| `derive.rs:35` | read/map | read: tests or masks the fact, seeds nothing |
| `gen_rest.rs:158` | seed | native-module `array.push` new length: perry-runtime array/push_pop.rs js_array_push_f64 (length re-read, `as f64`) |
| `gen_rest.rs:170` | seed | native-module receiver method count: perry-runtime returns `as f64` (gen_rest.rs:166 comment); NUM joined with the call's own result node |
| `gen_rest.rs:545` | seed | Expr::DateNow: perry-runtime date.rs js_date_now (f64 ms) |
| `gen_rest.rs:549` | seed | Expr::DateParse: perry-runtime date.rs js_date_parse (f64, NaN on failure = canonical f64 NaN) |
| `gen_rest.rs:556` | seed | Expr::DateUtc: perry-runtime date.rs js_date_utc (f64) |
| `gen_rest.rs:617` | seed | HIR Math intrinsics (prim_kids): codegen expr/math_simple.rs (llvm.*.f64) and perry-runtime math.rs js_math_* (f64 arithmetic) |
| `gen_rest.rs:637` | seed | `math_range` result (ToNumber of a Math operand): codegen expr/dispatch.rs lower_math_operand / js_math_to_number (f64) |
| `gen_rest.rs:644` | seed | ParseInt/ParseFloat/NumberCoerce: perry-runtime builtins/numbers.rs js_parse_int, js_parse_float, js_number_coerce (f64) |
| `gen_rest.rs:660` | seed | `+x` / ToNumber helper result: codegen unary lowering (fadd 0 / js_number_coerce, f64) |
| `builtins.rs:1000` | read/map | family/realm/const mapping: no value of its own; the seeds that use it are audited at their own rows |
| `builtins.rs:1640` | seed | timers: ret_tok(timer) + NUM, so NUM|timer object, never NUM-only. The NUM bit is a spurious over-approximation (REVIEW-p3 D2.5) |
| `builtins.rs:1801` | seed | Array.from map-callback index: perry-runtime array iteration passes the index `as f64` |
| `builtins.rs:1833` | seed | Array.from `this` length: NUM|UNDEF |
| `builtins.rs:1917` | seed | Math namespace calls: codegen math_simple.rs intrinsics; perry-runtime math.rs js_math_round/js_math_trunc/js_math_sign/js_math_log1p/js_math_random (f64 arithmetic). Split per member at B1 |
| `gen.rs:531` | seed | the shared NUM constant node (prim_const[0]); values flowing into it are audited at their own seed rows |
| `gen.rs:1286` | seed | `arguments.length`: codegen lowers the argument count `as f64` (sitofp) |
| `gen_call.rs:1063` | seed | js_iterator_delegate_kind (perry-runtime array/iterator_close.rs): returns a small kind code `as f64` |
| `gen_expr.rs:889` | seed | `>>>` result: codegen expr/binary.rs lshr + uitofp (f64) |
| `gen_expr.rs:899` | seed | unary `+`: codegen ToNumber (f64) |
| `gen_expr.rs:1522` | seed | numeric enum member value: an HIR numeric literal, lowered as a double constant |
| `solve.rs:46` | read/map | read: tests or masks the fact, seeds nothing |
| `solve.rs:236` | read/map | read: tests or masks the fact, seeds nothing |
| `solve.rs:776` | read/map | read: tests or masks the fact, seeds nothing |
| `solve.rs:790` | read/map | read: tests or masks the fact, seeds nothing |
| `solve.rs:1828` | read/map | read: tests or masks the fact, seeds nothing |
| `solve.rs:1840` | read/map | read: tests or masks the fact, seeds nothing |
| `solve.rs:1851` | read/map | read: tests or masks the fact, seeds nothing |
| `bindings.rs:193` | read/map | read: tests or masks the fact, seeds nothing |
| `bindings.rs:224` | read/map | read: tests or masks the fact, seeds nothing |
| `bindings.rs:392` | read/map | read: tests or masks the fact, seeds nothing |
| `bindings.rs:470` | read/map | read: tests or masks the fact, seeds nothing |
| `bindings.rs:589` | read/map | read: tests or masks the fact, seeds nothing |
| `bindings.rs:682` | read/map | read: tests or masks the fact, seeds nothing |
| `bindings.rs:683` | read/map | read: tests or masks the fact, seeds nothing |
| `bindings.rs:746` | read/map | read: tests or masks the fact, seeds nothing (I32 at rest: a Number field's rep) |
| `bindings.rs:850` | read/map | read: tests or masks the fact, seeds nothing (I32 at rest: the `# rest` probe) |
| `signatures.rs:389` | read/map | read: tests or masks the fact, seeds nothing |
| `dump.rs:470` | read/map | read: tests or masks the fact, seeds nothing |
| `records.rs:448` | read/map | read: R3's immediate-field mask (value records), seeds nothing |
| `solve/arr_len.rs:42` | seed | array birth length `new Array(k)`: the header length `uitofp` |
| `solve/lookup.rs:1289` | seed | array `.length` through the chain: header length `uitofp` (array/indexing.rs js_array_length -> u32) |
| `solve/fields.rs:40` | seed | RegExp `lastIndex` at birth: the Number 0 |
| `solve/native_value.rs:171` | read/map | read: tests or masks the fact, seeds nothing |
| `solve/apply.rs:1309` | read/map | read: tests or masks the fact, seeds nothing |
| `solve/apply.rs:1337` | read/map | read: tests or masks the fact, seeds nothing |
| `solve/apply.rs:1456` | read/map | read: tests or masks the fact, seeds nothing |
| `solve/apply.rs:1472` | read/map | read: tests or masks the fact, seeds nothing |
| `solve/apply.rs:1473` | seed | numeric result of an arithmetic or `+` operator: codegen expr/binary.rs and expr/unary.rs lower it inline (fadd/fsub/fmul/fdiv, toint32 + sitofp), and the dynamic arm calls perry-runtime value/dynamic_arith.rs js_dynamic_string_or_number_add / dynamic_add_non_plain, which decodes an INT32 operand (`as_int32() as f64`) and returns `a_num + b_num` (f64) |
| `solve/apply.rs:1486` | seed | numeric result of an arithmetic or `+` operator: codegen expr/binary.rs and expr/unary.rs lower it inline (fadd/fsub/fmul/fdiv, toint32 + sitofp), and the dynamic arm calls perry-runtime value/dynamic_arith.rs js_dynamic_string_or_number_add / dynamic_add_non_plain, which decodes an INT32 operand (`as_int32() as f64`) and returns `a_num + b_num` (f64) |
| `solve/apply.rs:1492` | seed | numeric result of an arithmetic or `+` operator: codegen expr/binary.rs and expr/unary.rs lower it inline (fadd/fsub/fmul/fdiv, toint32 + sitofp), and the dynamic arm calls perry-runtime value/dynamic_arith.rs js_dynamic_string_or_number_add / dynamic_add_non_plain, which decodes an INT32 operand (`as_int32() as f64`) and returns `a_num + b_num` (f64) |
| `solve/apply.rs:1502` | seed | numeric result of an arithmetic or `+` operator: codegen expr/binary.rs and expr/unary.rs lower it inline (fadd/fsub/fmul/fdiv, toint32 + sitofp), and the dynamic arm calls perry-runtime value/dynamic_arith.rs js_dynamic_string_or_number_add / dynamic_add_non_plain, which decodes an INT32 operand (`as_int32() as f64`) and returns `a_num + b_num` (f64) |
| `solve/apply.rs:1503` | seed | numeric result of an arithmetic or `+` operator: codegen expr/binary.rs and expr/unary.rs lower it inline (fadd/fsub/fmul/fdiv, toint32 + sitofp), and the dynamic arm calls perry-runtime value/dynamic_arith.rs js_dynamic_string_or_number_add / dynamic_add_non_plain, which decodes an INT32 operand (`as_int32() as f64`) and returns `a_num + b_num` (f64) |
| `solve/apply.rs:1513` | read/map | read: tests or masks the fact, seeds nothing |
| `gen/primitives.rs:64` | read/map | family/realm/const mapping: no value of its own; the seeds that use it are audited at their own rows |
| `gen/primitives.rs:100` | seed | computed enum member: NUM|STR, not NUM-only (no consumer drops a test) |
| `gen/primitives.rs:101` | seed | numeric enum member: a double constant |
| `gen/primitives.rs:110` | seed | number literal without an interval: a double constant |
| `gen/primitives.rs:121` | seed | number literal with its point interval: a double constant |
| `gen/primitives.rs:194` | seed | Math rule result (unary): codegen math intrinsic / js_math_* (f64) |
| `gen/primitives.rs:201` | seed | Math rule result (n-ary min/max): codegen logical_collections.rs lower_math_minmax_n (llvm.minimum/maximum) |
| `gen/narrow.rs:810` | seed | narrowing bound node: an interval-only node over program Numbers; no runtime value |
| `gen/glue.rs:228` | seed | arrayRecordIndex default: codegen array_record lowering, an i32 counter `sitofp` |
| `gen/glue.rs:230` | seed | arrayRecordLength: codegen array_record payload length `uitofp` |
| `gen/glue.rs:233` | seed | arrayRecordForUpdate: codegen `mode ? 0 : update` over program Numbers |
| `gen/glue.rs:182` | seed | `arrlazy` sabotage arm only (`PERRY_FACTS_SABOTAGE=arrlazy`): a deliberately false Array-of-Numbers claim for the S0 negative control; never seeded in a normal build |
| `builtins/loads.rs:292` | seed | string computed key: STR|NUM|UNDEF|BOOL, not NUM-only |
| `builtins/loads.rs:381` | seed | global `NaN`/`Infinity`/`undefined`: NUM|UNDEF; the constants are canonical doubles |
| `builtins/loads.rs:633` | seed | function `length`: closure arity `as f64`; joined with own fields (lookup.rs:221; REVIEW-p3 D2.1) |
| `builtins/loads.rs:667` | seed | class `length`: as above |
| `builtins/loads.rs:672` | seed | Array `length`: header length `uitofp` / array/indexing.rs js_array_length |
| `builtins/loads.rs:677` | seed | array computed key: UNDEF|NUM plus a function token, not NUM-only |
| `builtins/loads.rs:726` | seed | `.byteOffset` (B1-exposed): typedarray/access.rs, integer `as f64` |
| `builtins/loads.rs:731` | seed | buffer `size`/`length`/`byteLength`/`byteOffset`: typedarray/access.rs js_typed_array_length, integer `as f64` |
| `builtins/members.rs:81` | seed | Function.prototype `length` member: as loads.rs:633 |
| `builtins/members.rs:111` | seed | Array.prototype `length` member: as loads.rs:672 |
| `builtins/members.rs:147` | seed | String.prototype `length`: string length `as f64` |
| `builtins/members.rs:242` | seed | Map/Set `size`: perry-runtime map.rs/set.rs js_map_size/js_set_size, `as f64` |
| `builtins/members.rs:335` | read/map | family/realm/const mapping: no value of its own; the seeds that use it are audited at their own rows |
| `builtins/members.rs:663` | read/map | family/realm/const mapping: no value of its own; the seeds that use it are audited at their own rows |
| `builtins/members.rs:748` | read/map | family/realm/const mapping: no value of its own; the seeds that use it are audited at their own rows |
| `builtins/members.rs:1373` | read/map | family/realm/const mapping: no value of its own; the seeds that use it are audited at their own rows |
| `builtins/native_protocol.rs:61` | seed | EventEmitter `_` bookkeeping keys: NUM|UNDEF |
| `builtins/native_protocol.rs:675` | seed | EventEmitter listenerCount/getMaxListeners: perry-runtime event emitter counts `as f64` |
| `builtins/effects.rs:385` | read/map | family/realm/const mapping: no value of its own; the seeds that use it are audited at their own rows |
| `builtins/constructors.rs:187` | seed | typed-array elements: NUM|BIGINT, not NUM-only |
| `builtins/string_methods.rs:94` | seed | replacer argument: STR|NUM|UNDEF, not NUM-only |
| `builtins/string_methods.rs:215` | seed | charCodeAt/indexOf/lastIndexOf: perry-runtime string/char_ops.rs js_string_char_code_at, string/slice_ops.rs js_string_index_of/js_string_last_index_of (i32/u16 `as f64`; NaN for charCodeAt out of range = canonical NaN) |
| `builtins/string_methods.rs:216` | seed | codePointAt: NUM|UNDEF; string/char_ops.rs js_string_code_point_at |
| `builtins/string_methods.rs:282` | seed | localeCompare: string/compare.rs js_string_locale_compare (-1/0/1 `as f64`) |
| `builtins/string_methods.rs:350` | seed | @@search: RegExp search index `as f64` (string regexp path) |
| `builtins/string_methods.rs:359` | seed | match record `index`: regexp match index `as f64` |
| `builtins/string_methods.rs:436` | seed | split lastIndex: a modeled internal value (spec step), integer |
| `builtins/string_methods.rs:451` | seed | split capture index: modeled internal integer |
| `builtins/collections.rs:182` | seed | generator resume op code: a small integer the codegen generator step passes `as f64` |
| `builtins/collections.rs:574` | seed | entries() index field: the iteration index `as f64` |
| `builtins/collections.rs:588` | seed | callback index: array/iter_methods.rs, `as f64` |
| `builtins/collections.rs:700` | seed | constructed length: header length `uitofp` |
| `builtins/collections.rs:719` | seed | species length argument: an integer `as f64` |
| `builtins/collections.rs:885` | seed | push/pop/shift/unshift/splice new length on array-likes: array/push_pop.rs, array/splice_slice.rs; also above 2^32 a plain double (REVIEW-p3 p4) |
| `builtins/collections.rs:1003` | seed | index argument/result of the index-walking methods: array/search.rs js_array_indexOf_f64 -> i32, then codegen sitofp |
| `builtins/collections.rs:1050` | seed | push(spread) result: as collections.rs:885 |
| `builtins/collections.rs:1177` | seed | index-returning array methods (indexOf/findIndex/...): array/search.rs, array/iter_methods.rs js_array_findIndex (i32 `as f64`) |
| `builtins/collections.rs:1194` | seed | lastIndexOf/findIndexOf: NUM|BOOL, not NUM-only |
| `builtins/collections.rs:1410` | seed | Date getters/valueOf: perry-runtime date.rs js_date_get_* (f64) |
| `builtins/collections.rs:1438` | seed | Date setters' result: date.rs (f64 time value) |
| `builtins/collections.rs:1536` | seed | TextEncoder.encode result elements: as gen_rest/web.rs:36 |
| `builtins/collections.rs:1543` | seed | encodeInto read/written: as gen_rest/web.rs:51 |
| `builtins/collections.rs:1561` | seed | Timer @@toPrimitive: the timer id `as f64` |
| `builtins/typed_array_methods.rs:69` | seed | buffer length: typedarray/access.rs, integer `as f64` |
| `builtins/typed_array_methods.rs:76` | seed | fresh typed-array elements: NUM|BIGINT, not NUM-only |
| `builtins/typed_array_methods.rs:91` | seed | typed-array callback index: typedarray/iterate.rs, `as f64` |
| `builtins/typed_array_methods.rs:93` | seed | typed-array element: NUM|BIGINT, not NUM-only |
| `builtins/typed_array_methods.rs:113` | seed | at(): NUM|BIGINT|UNDEF, not NUM-only |
| `builtins/typed_array_methods.rs:142` | seed | indexOf/lastIndexOf/findIndex results: perry-runtime object/native_call_method/typed_array.rs:271 (`js_array_indexOf_jsvalue(..) as f64`, `js_array_last_index_of_jsvalue(..) as f64`), reached from object/typed_array_proto_thunks.rs ta_index_of_thunk / ta_last_index_of_thunk |
| `builtins/typed_array_methods.rs:175` | seed | find/findLast: NUM|BIGINT|UNDEF, not NUM-only |
| `builtins/typed_array_methods.rs:176` | seed | other typed-array Number results (length, reduce of numbers...): typedarray/*.rs, f64 |
| `derive/element.rs:120` | read/map | read: tests or masks the fact, seeds nothing |
| `gen_rest/web.rs:36` | seed | TextEncoder.encode result elements: a Uint8Array; typed-array element reads canonicalize (perry-runtime typedarray/access.rs; REVIEW-p3 t4/t5) |
| `gen_rest/web.rs:51` | seed | TextEncoder.encodeInto `read`/`written`: perry-runtime text.rs, counts `as f64` |
| `gen_rest/builtin_objects.rs:375` | seed | typed-array view elements (shared buffer): typed-array reads canonicalize NaN (typedarray/access.rs; t4/t5) |
| `gen_rest/builtin_objects.rs:393` | seed | typed-array elements: as above |
| `gen_rest/nodes.rs:33` | seed | typed-array elements: NUM|BIGINT, not NUM-only |
| `gen_rest/collections.rs:15` | seed | push(...spread) new length: array/push_pop.rs, `as f64` |
| `gen_rest/collections.rs:43` | seed | copyWithin operand ToNumber: operand conversion, not a value the program reads |
| `gen_rest/collections.rs:48` | seed | copyWithin end operand ToNumber: as above |
| `gen_rest/collections.rs:63` | seed | callback index argument: perry-runtime array/iter_methods.rs passes the index `as f64` |
| `gen_rest/collections.rs:152` | seed | groupBy callback index: as above |
| `gen_rest/collections.rs:213` | seed | callback index (from/forEach): as above |
| `canonical_numbers.rs:49` | read/map | read: tests or masks the fact, seeds nothing (P3 B1's element-value test) |
| `canonical_numbers.rs:50` | read/map | read: tests or masks the fact, seeds nothing (P3 B1's element-value test) |
| `zero_sign.rs:420` | read/map | read: a node holding Numbers whose uses the generator does not see is a zero-sign seed; seeds nothing (PR D) |
| `zero_sign.rs:527` | read/map | read: counts the tolerated Number nodes; seeds nothing (PR D) |

## INT32-box producers claimed by code comments (CWN, P3 B1)
Two runtime comments claimed live INT32-tagged Number producers. Neither is live: no code path writes an INT32 word for
a Number, and the comments were stale (fixed in P3 B1).

| comment | code proof | running proof (qb9, main dda8f1e84 vs node 26.8.1) |
|---|---|---|
| claim at `perry-runtime/src/closure/dispatch/value_call.rs:410` "V8 supplies INT32-tagged numbers" | The V8 embedding (perry-jsruntime) was removed in #1696. `js_create_callback` is now only a stub that returns `undefined` (`closure/v8_stubs.rs:232`, and the weak Mach-O definition at :103), so nothing reaches `js_closure_v8_callback` with V8 values. The other entry, `js_closure_call_array`, is called by perry-ffi `Closure::call` (args are `JsValue`s, and `from_int32` is `n as f64`) and by ext-http (array elements). | `v8.deserialize(v8.serialize(5))`, an array `[5, -7, 0, 2147483647]` and an object `{a: 5, b: {c: 5}}`: `Object.is(x, 5)`, `x === 5`, `typeof x`, `x + 0.5` and the `Float64Array` round trip all match node. Raw bits: `v8_serde_canonical_tests.rs` asserts that a wire int32 (`'I'` + zigzag) deserializes to exactly `(n as f64).to_bits()` (`v8_serde.rs:727 int_to_value` is `v as f64`). |
| claim at `perry-runtime/src/json/stringify_scalars.rs:33` "Integer columns from the sqlite binding" | All sqlite integer readers return `JSValue::number(n as f64)`: node:sqlite `bind.rs:400 node_sqlite_integer_value`; user-function arguments `sqlite_callbacks.rs:124 integer_value_checked`; bun:sqlite and better `bind.rs:18 sqlite_value_to_jsvalue`; the ext crate `perry-ext-better-sqlite3/src/lib.rs:90` via perry-ffi `from_int32` = `n as f64`. | node:sqlite `INTEGER` rows (5, -7, 2147483647) through `.all()` and `.get()`: all checks match node. Raw bits: the `bind.rs` test `integer_columns_are_raw_double_numbers` asserts `(n as f64).to_bits()` for both `sqlite_value_to_jsvalue` and `node_sqlite_integer_value`. |
