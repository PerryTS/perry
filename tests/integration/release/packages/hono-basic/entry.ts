import { Hono } from "hono";

const app = new Hono();

app.get("/", (c) => {
    return c.json({ hello: "world" });
});

app.get("/users/:id", (c) => {
    const id = c.req.param("id");
    return c.json({ id, name: `User ${id}` });
});

app.notFound((c) => c.text("not found", 404));

async function main() {
    const r1 = await app.fetch(new Request("http://x/"));
    console.log(`r1.status=${r1.status}`);
    console.log(`r1.body=${await r1.text()}`);

    const r2 = await app.fetch(new Request("http://x/users/42"));
    console.log(`r2.status=${r2.status}`);
    console.log(`r2.body=${await r2.text()}`);

    const r3 = await app.fetch(new Request("http://x/missing"));
    console.log(`r3.status=${r3.status}`);
    console.log(`r3.body=${await r3.text()}`);
}

main();
