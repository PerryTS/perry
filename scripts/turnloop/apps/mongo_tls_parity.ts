// P12 acceptance: the `mongodb` surface over **TLS**, run identically on Perry
// and on Node 26.5.1 with the real npm `mongodb`, against the same server with
// `--tlsMode preferTLS`.
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

  const col = client.db(DB).collection("p12_mongo_tls");
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
  console.log("find-one:", JSON.stringify(await col.findOne({ _id: "b" })));

  // A document wider than one TLS record, so an OP_MSG has to be reassembled
  // across record boundaries rather than arriving whole.
  await col.insertOne({ _id: "wide", blob: "z".repeat(70000) });
  const wide = await col.findOne({ _id: "wide" });
  console.log("wide-len:", wide === null ? -1 : (wide as { blob: string }).blob.length);

  console.log("delete:", (await col.deleteMany({})).deletedCount);
  await client.close();
  console.log("done");
}

main().catch((e) => {
  console.log("FAILED:", e instanceof Error ? e.message : String(e));
  process.exitCode = 1;
});
