// demonstrates: importing built-in stdlib npm packages (project-config.md)
// docs: docs/src/getting-started/project-config.md
// platforms: macos, linux, windows
// requires: auto-optimize
// run: false

// These four imports are Perry's most-used built-in stdlib shims:
// fastify (HTTP server), mysql2 (db), ioredis (Redis), bcrypt (password
// hashing). They're compiled to native code via Perry's per-package
// implementations — no `compilePackages` needed.
//
// `// run: false` because each one needs a live external service (DB,
// Redis, network port) to actually do anything; the binary still has to
// link cleanly, which is the drift check we want.

import fastify from "fastify"
import mysql from "mysql2/promise"
import Redis from "ioredis"
import bcrypt from "bcrypt"

const app = fastify({ logger: false })
const db = mysql.createPool({ host: "localhost", user: "root", database: "test" })
const redis = new Redis()
const hashed = await bcrypt.hash("hunter2", 10)

console.log(typeof app, typeof db, typeof redis, hashed.length)
