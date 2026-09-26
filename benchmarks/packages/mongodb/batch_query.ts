// mongodb: insertMany of 20-doc batches + a filtered/sorted find().toArray()
// + countDocuments per iteration.
import { MongoClient } from "mongodb";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";
import { port, HOST } from "../_lib/env.ts";

const it = iters(500, 50);
header("mongodb/batch_query", "mongodb", it);

async function main(): Promise<void> {
  const client = new MongoClient("mongodb://" + HOST + ":" + port("PKG_BENCH_MONGO_PORT") + "/?directConnection=true");
  await client.connect();
  const col = client.db("pkgbench").collection("items_bq");
  const once = async (i: number, h: number): Promise<number> => {
    const docs = [];
    for (let k = 0; k < 20; k++) docs.push({ b: i, k: k, v: (i * 20 + k) % 97, s: "s" + k });
    await col.insertMany(docs);
    const rows = await col.find({ b: i, v: { $gt: 40 } }).sort({ v: -1 }).project({ _id: 0, k: 1, v: 1 }).toArray();
    const cnt = await col.countDocuments({ b: i });
    let s = cnt + ":" + rows.length;
    for (const r of rows) s += "," + r.k + "=" + r.v;
    return fnv(h, s);
  };
  await col.deleteMany({});
  let h = FNV_SEED;
  for (let i = 0; i < it.warm; i++) h = await once(i, h);
  await col.deleteMany({});
  h = FNV_SEED;
  for (let i = 0; i < it.n; i++) h = await once(i, h);
  await col.drop();
  await client.close();
  console.log("checksum " + hex(h));
}
main().catch((e) => { console.log("ERROR " + (e && e.message)); process.exitCode = 1; });
