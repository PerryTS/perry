// Integration test for Fastify (issue #174). Runs a small server that
// scripts/run_fastify_tests.sh launches in the background, curls, and
// asserts the response bodies for each route. Port is read from argv
// so the harness can pick a free port to avoid CI conflicts.
import fastify from "fastify";

const port = parseInt(process.argv[2] || "3456");

const app = fastify();

app.get("/hello", async (_request, _reply) => {
  return { hello: "world" };
});

app.get("/users/:id", async (request, _reply) => {
  const { id } = request.params;
  return { id: id, name: "User " + id };
});

app.post("/echo", async (request, reply) => {
  reply.code(201);
  return { received: request.body };
});

app.listen({ port: port }, () => {
  // Sentinel line the harness waits for before starting curl assertions.
  console.log("ready port=" + port);
});
