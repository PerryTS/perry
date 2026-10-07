A method call whose name some class also declares no longer demotes every
other receiver to by-name dispatch. In an oversized (full-outline) module the
class-id tower's collapse called `js_native_call_method` for every receiver;
those calls now take the universal method site, whose memo is keyed on the
receiver's shape, so a RegExp, a namespace object or a plain object is served
by one shape compare and one call (Claude Code's string-width: `oR_.test(O)`
7,969 -> 1,668 and `g54.default()` 579 -> 91 instructions per call in the
reproduction). In ordinary modules a tower too wide for the shape probe now
also sends receivers no implementor claims to the method site. Removes
`emit_collapsed_instance_dispatch` and the by-name collapse arms.
