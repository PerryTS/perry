// P7 acceptance: the `mongodb` surface, run identically on Perry and on
// Node 26.5.1 with the real npm `mongodb`, against the same MongoDB server.
//
// The URI is a direct, single-server, plaintext `mongodb://host:port` — the one
// configuration P7 migrated. `+srv`, TLS, several hosts, `replicaSet=` and
// `compressors=` all decline to the existing driver, so a fixture using any of
// them would be measuring the legacy path.
//
// `_id` is printed as a plain field only where this file supplies it, because
// a server-generated ObjectId differs between runs and between engines.
//
//   MONGO_HOST=127.0.0.1 MONGO_PORT=57017 MONGO_DB=perry_test
//
// parity-skip: requires a live MongoDB fixture
import { MongoClient } from "mongodb";

const HOST = process.env.MONGO_HOST ?? "127.0.0.1";
const PORT = process.env.MONGO_PORT ?? "27017";
const DB = process.env.MONGO_DB ?? "test";

async function main(): Promise<void> {
  const client = new MongoClient(`mongodb://${HOST}:${PORT}`);
  await client.connect();

  const db = client.db(DB);
  const col = db.collection("p7_mongo");

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
