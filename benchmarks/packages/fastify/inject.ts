// fastify: route + JSON serialize via app.inject() (light-my-request, no
// socket) — GET with a path param and POST with a JSON body per iteration.
import Fastify from "fastify";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(5000, 300);
header("fastify/inject", "fastify", it);

async function main(): Promise<void> {
  const app = Fastify({ logger: false });
  app.get("/user/:id", async (req: any) => ({ id: req.params.id, ok: true, tags: ["a", "b"] }));
  app.post("/echo", async (req: any) => ({ got: req.body.n, name: req.body.name }));
  await app.ready();
  const once = async (i: number, h: number): Promise<number> => {
    const r1 = await app.inject({ method: "GET", url: "/user/" + (i % 100) });
    const r2 = await app.inject({ method: "POST", url: "/echo", payload: { n: i, name: "x" + (i % 7) } });
    return fnv(fnv(h, r1.statusCode + r1.body), r2.statusCode + r2.body);
  };
  let h = FNV_SEED;
  for (let i = 0; i < it.warm; i++) h = await once(i, h);
  h = FNV_SEED;
  for (let i = 0; i < it.n; i++) h = await once(i, h);
  await app.close();
  console.log("checksum " + hex(h));
}
main().catch((e) => { console.log("ERROR " + (e && e.message)); process.exitCode = 1; });
