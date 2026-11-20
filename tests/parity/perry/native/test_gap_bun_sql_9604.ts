// Bun.SQL SQLite tagged-template compatibility (#9604).
import { SQL } from "bun";
import { unlinkSync } from "node:fs";

declare function gc(): void;

function forceGc(): void {
  if (typeof gc === "function") gc();
}

async function main(): Promise<void> {
  let connected = 0;
  let closed = 0;
  let closeHadError = false;
  const sql = new SQL(":memory:", {
    onconnect(client: unknown) {
      connected++;
      console.log("connected:", typeof client === "function");
    },
    onclose(client: unknown, error: unknown) {
      closed++;
      closeHadError = error !== undefined;
      console.log("close-client:", typeof client === "function");
    },
  });
  // Exercise the callable client's captures and dynamic method properties
  // after a moving collection, not only at their freshly allocated address.
  forceGc();

  console.log(
    "shape:",
    typeof sql,
    typeof sql.begin,
    typeof sql.reserve,
    typeof sql.close,
  );

  const create = sql`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT)`;
  console.log("thenable:", typeof create.then);
  await create;

  const hostile = "one'); DROP TABLE items; --";
  await sql`INSERT INTO items (name) VALUES (${hostile}), (${"two"})`;
  const rows = await sql`SELECT id, name FROM items ORDER BY id`;
  console.log(
    "rows:",
    rows.length,
    rows[0].id,
    rows[0].name === hostile,
    rows[1].name,
  );

  const result = await sql.begin(async (tx: typeof sql) => {
    await Promise.resolve();
    forceGc();
    await tx`INSERT INTO items (name) VALUES (${"three"})`;
    return "committed";
  });
  console.log("commit:", result, (await sql`SELECT COUNT(*) AS n FROM items`)[0].n);

  let rollbackMessage = "";
  try {
    await sql.begin(async (tx: typeof sql) => {
      await tx`INSERT INTO items (name) VALUES (${"rolled back"})`;
      await Promise.resolve();
      forceGc();
      throw new Error("rollback");
    });
  } catch (error) {
    rollbackMessage = (error as Error).message;
  }
  console.log(
    "rollback:",
    rollbackMessage,
    (await sql`SELECT COUNT(*) AS n FROM items`)[0].n,
  );

  await sql.begin(async (outer: typeof sql) => {
    await outer.begin(async (middle: typeof sql) => {
      try {
        await middle.begin(async (inner: typeof sql) => {
          await inner`INSERT INTO items (name) VALUES (${"nested rollback"})`;
          forceGc();
          throw new Error("nested rollback");
        });
      } catch {}
      await middle`INSERT INTO items (name) VALUES (${"four"})`;
    });
  });
  console.log("nested:", (await sql`SELECT COUNT(*) AS n FROM items`)[0].n);

  const reserved = await sql.reserve();
  console.log(
    "reserve:",
    (await reserved`SELECT name FROM items WHERE id = ${3}`)[0].name,
  );
  reserved.release();

  await sql.close({ timeout: 0 });
  console.log("closed:", connected, closed, closeHadError);

  try {
    await sql`SELECT 1`;
  } catch (error) {
    console.log("after-close:", error instanceof Error);
  }

  const filename = `/tmp/perry-bun-sql-${process.pid}.db`;
  const writable = new SQL(`sqlite://${filename}?mode=rwc`);
  await writable`CREATE TABLE mode_check (value TEXT)`;
  await writable`INSERT INTO mode_check (value) VALUES (${"persisted"})`;
  await writable.close({ timeout: 0 });

  const readonly = new SQL(`file://${filename}?mode=ro`);
  const persisted = await readonly`SELECT value FROM mode_check`;
  let writeRejected = false;
  try {
    await readonly`INSERT INTO mode_check (value) VALUES (${"rejected"})`;
  } catch {
    writeRejected = true;
  }
  console.log("readonly:", persisted[0].value, writeRejected);
  await readonly.close({ timeout: 0 });
  unlinkSync(filename);
}

await main();
