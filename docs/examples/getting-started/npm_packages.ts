// demonstrates: importing built-in stdlib npm packages (project-config.md)
// docs: docs/src/getting-started/project-config.md
// platforms: macos, linux, windows
// requires: auto-optimize
// run: false

// Four common npm packages: fastify (HTTP server), mysql2 (db), ioredis
// (Redis), bcrypt (password hashing). With no `perry.compilePackages` entry,
// Perry compiles each installed package's real source to native code (bcrypt,
// a Node native addon, routes to Perry's perry-ext-bcrypt wrapper). They must
// be installed: the doc-tests job runs `npm ci` at the repo root, whose
// devDependencies list them.
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
