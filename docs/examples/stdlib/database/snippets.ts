// demonstrates: per-driver database client snippets shown in
//   docs/src/stdlib/database.md
// docs: docs/src/stdlib/database.md
// platforms: macos, linux
// Windows excluded: this complete-file example uses try/catch, which the
// default Windows RS4GC pipeline intentionally refuses until WinEH funclet
// statepoints are supported (#7354).
// run: false

// Each ANCHOR block below is the exact code that the database docs page
// renders inline (via {{#include ... :NAME}}). The whole file is compiled
// and linked by the doc-tests harness — `run: false` because every snippet
// connects to a database server that isn't available in CI. Compile + link
// is the contract: it catches client API-shape drift, which is what bites
// users (e.g. an `execute` overload changing from one positional arg to
// two would break user code today and is exactly the kind of regression
// these tests guard against).
//
// Each snippet is wrapped in its own async function so import-time work
// (e.g. `await client.connect()`) doesn't run at module load.

// ANCHOR: mysql
import mysql from "mysql2/promise"

async function mysqlExample(): Promise<void> {
    const connection = await mysql.createConnection({
        host: "localhost",
        user: "root",
        password: "password",
        database: "mydb",
    })

    const [rows] = await connection.execute("SELECT * FROM users WHERE id = ?", [1])
    console.log(rows)

    await connection.end()
}
// ANCHOR_END: mysql

// ANCHOR: postgres
import { Client } from "pg"

async function postgresExample(): Promise<void> {
    const client = new Client({
        host: "localhost",
        port: 5432,
        user: "postgres",
        password: "password",
        database: "mydb",
    })

    await client.connect()
    const result = await client.query("SELECT * FROM users WHERE id = $1", [1])
    console.log(result.rows)
    await client.end()
}
// ANCHOR_END: postgres

// ANCHOR: sqlite
import Database from "better-sqlite3"

function sqliteExample(): void {
    const db = new Database("mydb.sqlite")

    db.exec(`
      CREATE TABLE IF NOT EXISTS users (
        id INTEGER PRIMARY KEY,
        name TEXT,
        email TEXT
      )
    `)

    const insert = db.prepare("INSERT INTO users (name, email) VALUES (?, ?)")
    insert.run("Perry", "perry@example.com")

    const users = db.prepare("SELECT * FROM users").all()
    console.log(users)
}
// ANCHOR_END: sqlite

// ANCHOR: mongodb
import { MongoClient } from "mongodb"

async function mongoExample(): Promise<void> {
    const client = new MongoClient("mongodb://localhost:27017")
    await client.connect()

    const db = client.db("mydb")
    const users = db.collection("users")

    await users.insertOne({ name: "Perry", email: "perry@example.com" })
    const user = await users.findOne({ name: "Perry" })
    console.log(user)

    await client.close()
}
// ANCHOR_END: mongodb

// ANCHOR: redis
import Redis from "ioredis"

async function redisExample(): Promise<void> {
    const redis = new Redis()

    await redis.set("key", "value")
    const value = await redis.get("key")
    console.log(value) // "value"

    await redis.del("key")
    await redis.quit()
}
// ANCHOR_END: redis

// Touch every example so unused-import elimination doesn't strip the imports.
const _keep = [mysqlExample, postgresExample, sqliteExample, mongoExample, redisExample]
console.log(`db-snippets: ${_keep.length}`)
