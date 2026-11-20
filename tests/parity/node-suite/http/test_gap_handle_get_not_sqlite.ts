// A `.get()` / `.close()` on a handle-backed object that is not a SQLite
// statement or database (here a fetch Response's headers) must reach its own
// method. The SQLite arm of the dynamic handle dispatch claimed every handle
// for `get`/`all`/`run`/`raw`/`prepare`/`exec`/`close`; since its entry
// points throw for a handle they do not own, `response.headers.get(...)` threw
// "The database connection is not open". The program also uses SQLite, so
// the SQLite arm is linked even when the libraries are built per program.
import { createServer } from "node:http";
import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE t (v INTEGER)");
db.prepare("INSERT INTO t VALUES (?)").run(7);
console.log("sqlite", JSON.stringify(db.prepare("SELECT v FROM t").get()));

const server = createServer((_req, res) => {
    res.setHeader("cache-control", "max-age=300");
    res.setHeader("etag", '"v1"');
    res.end("ok");
});
server.listen(0, async () => {
    const { port } = server.address() as { port: number };
    const response: any = await fetch(`http://127.0.0.1:${port}/`);
    const headers: any = response.headers;
    console.log("cache-control", headers.get("cache-control"));
    console.log("etag", headers.get("etag"));
    console.log("missing", headers.get("x-missing"));
    console.log("body", await response.text());
    server.close();
    db.close();
});
