// P12 acceptance: the `mongodb` surface over **TLS**, run identically on Perry
// and on Node 26.5.1 with the real npm `mongodb`, against the same server with
// `--tlsMode preferTLS`.
//
// The body below is `mongo_parity.ts`'s, unchanged, and that is the point: the
// ONLY difference between the two files is `?tls=true` in the URI. Every
// divergence that file documents as pre-existing is pre-existing here too, so
// a diff between the two runs is about the transport and nothing else. Writing
// a fresh fixture instead cost an afternoon: a `?? Object.keys(...)` and a
// `as { blob: string }` cast both hit TypeScript-subset gaps that had nothing
// to do with TLS and presented as the connection failing.
//
// `tls=true` used to decline to the `mongodb` crate, which is the only one of
// the four legacy database paths that really could do TLS. So unlike the other
// three this row is a *migration* rather than a repair: the configuration
// worked before and has to keep working, byte for byte.
//
// TLS starts at connect time, before the `hello` — and therefore before the
// speculative SCRAM a credentialed URI carries in that same document.
//
// The trust root reaches both engines through `NODE_EXTRA_CA_CERTS`, not
// through a `tlsCAFile` URI option: that key makes `turnloop_mongodb`'s URI
// parser refuse the whole URI, which would send this fixture back to the
// legacy driver and measure nothing.
//
//   MONGO_HOST=127.0.0.1 MONGO_PORT=57017 MONGO_DB=perry_test
//   NODE_EXTRA_CA_CERTS=<ca.crt>
//
// parity-skip: requires a live TLS-enabled MongoDB fixture
import { MongoClient } from "mongodb";

const HOST = process.env.MONGO_HOST ?? "127.0.0.1";
const PORT = process.env.MONGO_PORT ?? "27017";
const DB = process.env.MONGO_DB ?? "test";

async function main(): Promise<void> {
  const client = new MongoClient(`mongodb://${HOST}:${PORT}/?tls=true`);
  await client.connect();

  const db = client.db(DB);
  const col = db.collection("p12_mongo_tls");

  await col.deleteMany({});

  const one = await col.insertOne({ _id: "a", n: 1, name: "alpha", flag: true });
  console.log("insert-one-acknowledged:", one.acknowledged === true || one.insertedId !== undefined);

  const many = await col.insertMany([
    { _id: "b", n: 2, name: "bêta", flag: false },
    { _id: "c", n: 3, name: "gamma", flag: true },
  ]);
  console.log("insert-many-count:", many.insertedCount ?? Object.keys(many.insertedIds ?? {}).length);

  console.log("count:", await col.countDocuments({}));
  console.log("count-filtered:", await col.countDocuments({ flag: true }));

  const found = await col.findOne({ _id: "b" });
  console.log("find-one:", JSON.stringify(found));
  console.log("find-one-missing:", JSON.stringify(await col.findOne({ _id: "zzz" })));

  const all = await col.find({}).toArray();
  all.sort((x, y) => ((x as { _id: string })._id < (y as { _id: string })._id ? -1 : 1));
  console.log("find-all:", JSON.stringify(all));

  const filtered = await col.find({ flag: true }).toArray();
  console.log("find-filtered-count:", filtered.length);

  const upd = await col.updateOne({ _id: "a" }, { $set: { name: "alpha-2" } });
  console.log("update-one-modified:", upd.modifiedCount);
  console.log("after-update:", JSON.stringify(await col.findOne({ _id: "a" })));

  const updMany = await col.updateMany({ flag: true }, { $set: { touched: 1 } });
  console.log("update-many-modified:", updMany.modifiedCount);

  const del = await col.deleteOne({ _id: "c" });
  console.log("delete-one:", del.deletedCount);
  console.log("count-after-delete:", await col.countDocuments({}));

  // More documents than one OP_MSG batch carries (the server's default is 101),
  // so reading them all requires following the cursor with `getMore`. The
  // turnloop path does; whether it does *correctly* cannot be checked from
  // here, because `find().toArray()` resolves an empty string on both Perry
  // arms (see the P7 report's defect list) — so this checks what Perry can
  // observe, which is the server's own count after a 250-document insert.
  await col.deleteMany({});
  const bulk: Array<{ _id: string; k: number }> = [];
  for (let i = 0; i < 250; i++) bulk.push({ _id: `k${String(i).padStart(3, "0")}`, k: i });
  await col.insertMany(bulk);
  console.log("bulk-count:", await col.countDocuments({}));
  console.log("bulk-count-filtered:", await col.countDocuments({ k: 42 }));
  const one249 = await col.findOne({ _id: "k249" });
  console.log("bulk-last:", JSON.stringify(one249));

  await col.deleteMany({});
  console.log("count-after-clear:", await col.countDocuments({}));

  await client.close();
  console.log("done");
}

main().catch((e) => {
  console.log("FAILED:", e instanceof Error ? e.message : String(e));
  process.exitCode = 1;
});
