// Issue #536 regression: a user-defined class named `Pool`, `Client`, or
// `MongoClient` (or one imported from a TS-source npm package like
// `@perryts/mysql`) was unconditionally misclassified as `pg`/`mongodb`,
// routing later `.query()` / `.end()` etc. calls to `js_pg_pool_*` runtime
// symbols and failing at link time. The lookup gating in `destructuring.rs`
// + `lower.rs` now restricts the hardcoded `Pool`/`Client`/`MongoClient`
// shortcut to actual native-module imports.

class Pool {
    private url: string;
    constructor(opts: { url: string }) { this.url = opts.url; }
    async query(sql: string): Promise<string> { return `result for ${sql} on ${this.url}`; }
    async end(): Promise<void> {}
}

class Client {
    private name: string;
    constructor(name: string) { this.name = name; }
    greet(): string { return `hello from ${this.name}`; }
}

class MongoClient {
    private uri: string;
    constructor(uri: string) { this.uri = uri; }
    db(name: string): string { return `${this.uri}/${name}`; }
}

async function main() {
    const pool = new Pool({ url: "mysql://x" });
    console.log(await pool.query("SELECT 1"));
    await pool.end();

    const c = new Client("alice");
    console.log(c.greet());

    const m = new MongoClient("mongodb://x");
    console.log(m.db("test"));
}

main();
