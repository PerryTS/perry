// axios: sequential GET + JSON decode against the harness's local HTTP
// server (a separate Node process), keep-alive agent.
import axios from "axios";
import http from "node:http";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";
import { port, HOST } from "../_lib/env.ts";

const it = iters(2000, 200);
header("axios/get_json", "axios", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

async function main(): Promise<void> {
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  const client = axios.create({ baseURL: "http://" + HOST + ":" + port("PKG_BENCH_HTTP_PORT"), httpAgent: agent });
  const once = async (i: number, h: number): Promise<number> => {
    const r = await client.get("/json", { params: { i: i % 1000 } });
    return fnv(h, r.status + ":" + r.data.name + ":" + r.data.nested.n + ":" + r.data.tags.length);
  };
  let h = FNV_SEED;
  for (let i = 0; i < WARM; i++) h = await once(i, h);
  h = FNV_SEED;
  for (let i = 0; i < N; i++) h = await once(i, h);
  agent.destroy();
  console.log("checksum " + hex(h));
}
main().catch((e) => { console.log("ERROR " + (e && e.message)); process.exitCode = 1; });
