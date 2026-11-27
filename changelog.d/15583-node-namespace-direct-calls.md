Calls on node-core namespaces (`path.join(…)`, `(0, _path.join)(…)`, `path.posix.join(…)`,
`require("fs").readFileSync(…)`, `process.nextTick(…)`) call the native implementation
directly unless the program stores into that member; `path.join` joins in one pass. On
nextjs-hello this cuts instructions per request by about 6%. `process.hrtime.bigint` is
present on every `process.hrtime` value, `path.posix === path` on POSIX hosts, and
`import posix from "path/posix"` is `path.posix`.
