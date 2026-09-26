// mongodb: insertOne + findOne by _id round trips on a fresh collection.
import { MongoClient } from "mongodb";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";
import { port, HOST } from "../_lib/env.ts";

const it = iters(2000, 200);
header("mongodb/insert_find", "mongodb", it);

async function main(): Promise<void> {
  const client = new MongoClient("mongodb://" + HOST + ":" + port("PKG_BENCH_MONGO_PORT") + "/?directConnection=true");
  await client.connect();
  const col = client.db("pkgbench").collection("items_if");
  await col.deleteMany({});
  const once = async (i: number, h: number): Promise<number> => {
    await col.insertOne({ _id: "d" + i, n: i, name: "item-" + (i % 100), tags: ["a", "b"], nested: { q: i % 13 } } as any);
    const doc: any = await col.findOne({ _id: "d" + i } as any);
    return fnv(h, doc.n + ":" + doc.name + ":" + doc.tags.length + ":" + doc.nested.q);
  };
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
