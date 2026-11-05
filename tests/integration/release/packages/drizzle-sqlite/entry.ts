import { drizzle } from "drizzle-orm/better-sqlite3";
import { sqliteTable, integer, text } from "drizzle-orm/sqlite-core";
import { eq } from "drizzle-orm";
import Database from "better-sqlite3";

const sqlite = new Database(":memory:");
const db = drizzle(sqlite);

const users = sqliteTable("users", {
    id: integer("id").primaryKey(),
    name: text("name").notNull(),
    age: integer("age").notNull(),
});

// Drizzle migrations would normally happen via drizzle-kit; for this
// fixture we issue the CREATE TABLE directly.
sqlite.exec(`CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER NOT NULL)`);

db.insert(users).values([
    { id: 1, name: "alice", age: 30 },
    { id: 2, name: "bob", age: 25 },
    { id: 3, name: "carol", age: 35 },
]).run();

const all = db.select().from(users).all();
console.log(`count=${all.length}`);

const alice = db.select().from(users).where(eq(users.name, "alice")).all();
console.log(`alice.age=${alice[0].age}`);

sqlite.close();
