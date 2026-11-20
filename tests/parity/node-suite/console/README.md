# node:console granular parity suite

Focused Node.js parity coverage for `node:console`, curated from Node's `test/parallel/test-console*.js`, WPT console fixtures, and the Node-compatible parts of Deno's console tests.

The cases are TypeScript files that run under both Node (`--experimental-strip-types`) and Perry. They avoid Node's internal `common` helpers, stdio hijacking, Deno internals, TTY color negotiation, inspector state, and non-deterministic stack/timing details unless the parity runner already normalizes them.

## Coverage groups

This suite intentionally includes both strict output comparisons and shape/no-crash probes:

- import forms: namespace, default, named, and prefixless `console` imports
- output formatting: `%s`, `%d`, `%i`, `%f`, `%j`, `%o`, `%O`, `%c`, extra args, symbols, bigint, functions, errors, escaped strings, circular JSON
- counters/timers/groups: default and coerced labels, missing labels, duplicate labels, return values, and indentation
- structured output: `dir`, `dirxml`, `trace`, `clear`, `table`, inspector no-op helpers
- `Console` constructor: currently shape-oriented; stream-backed instance parity remains a future tightening area

## Known remaining inspect gaps

The remaining failing cases are tracked as shared `util.inspect` / object formatting work rather than console-specific dispatch gaps:

- `dir/depth-options` — #1199
- `dir/show-hidden-option` — #1200
- `dir/custom-inspect-visible` — #1201
- `output/function-format` — #1202
- `output/no-tostring-on-string-format` — #1203
- `output/json-circular` — #1204
