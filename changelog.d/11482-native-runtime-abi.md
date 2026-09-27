Fix native codegen/runtime ABI mismatches reported in #11408: preserve integer
predicate widths and box their results as JavaScript booleans, pass NaN-boxed
values through floating-point registers where the runtime expects them, and
supply missing optional parameters and array-assignment strictness. Correct
stale declarations for JSON, framework request predicates, Lodash, Sharp,
SQLite, Cheerio, and Nodemailer, plus HTTP Agent and WebSocket return types.

Add a required lint check for native register-class, width, arity, and missing
return mismatches. The audit reads dispatcher-added arguments and distinguishes
native-compatible platform definitions from the separate wasm pointer-width
worklist. Regression coverage checks emitted LLVM calls, omitted arguments,
boolean boxing, and strict/sloppy dynamic array stores.
