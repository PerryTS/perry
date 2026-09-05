// demonstrates: high-level "imports compile to native code" example shown
//   in docs/src/stdlib/overview.md
// docs: docs/src/stdlib/overview.md
// platforms: macos, linux, windows
// requires: auto-optimize
// run: false

// The overview page exists to show "these imports compile". So we just
// import the modules and reference them once each so no-unused-import
// warnings can't strip them. We don't actually start a server / connect
// to MySQL — that's why the banner is `run: false`: compile + link is the
// signal we care about, not runtime side-effects (which would require a
// live MySQL daemon and a free port, neither hermetic).

// ANCHOR: imports
import fastify from "fastify"
import mysql from "mysql2/promise"
// ANCHOR_END: imports

// Touch the imports so the compiler keeps them.
const _fastify_keep = typeof fastify
const _mysql_keep = typeof mysql
console.log(`overview-imports: fastify=${_fastify_keep} mysql=${_mysql_keep}`)
