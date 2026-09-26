// fastify: a real listening server (127.0.0.1, ephemeral port) driven by an
// in-process fetch() client — full socket + HTTP parse + serialize path.
import Fastify from "fastify";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(2000, 200);
header("fastify/listen_fetch", "fastify", it);

async function main(): Promise<void> {
  const app = Fastify({ logger: false });
  app.get("/item/:id", async (req: any) => ({ id: Number(req.params.id), name: "item-" + req.params.id }));
  await app.listen({ port: 0, host: "127.0.0.1" });
  const addr: any = app.server.address();
  const base = "http://127.0.0.1:" + addr.port;
  const once = async (i: number, h: number): Promise<number> => {
    const res = await fetch(base + "/item/" + (i % 500));
    const body: any = await res.json();
    return fnv(h, res.status + ":" + body.id + ":" + body.name);
  };
  let h = FNV_SEED;
  for (let i = 0; i < it.warm; i++) h = await once(i, h);
  h = FNV_SEED;
  for (let i = 0; i < it.n; i++) h = await once(i, h);
  await app.close();
  console.log("checksum " + hex(h));
}
main().catch((e) => { console.log("ERROR " + (e && e.message)); process.exitCode = 1; });
