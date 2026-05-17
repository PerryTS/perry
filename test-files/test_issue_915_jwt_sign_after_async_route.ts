// Issue #915 regression: jwt.sign after a resumed async Fastify route body
// must not route through the generic native-module ABI.

import Fastify from "fastify";
import jwt from "jsonwebtoken";

const port = parseInt(process.argv[2] || "18099");
const app = Fastify();

async function getThing(): Promise<string> {
  async function nested(): Promise<string> {
    return "x";
  }

  return await nested();
}

app.post("/go", async (_request, reply) => {
  await getThing();
  const token = jwt.sign({ sub: "x" }, "secret", { algorithm: "HS256" });
  reply.code(201);
  return { ok: true, len: token.length };
});

app.listen({ host: "127.0.0.1", port: port }, () => {
  console.log("ready port=" + port);
});
