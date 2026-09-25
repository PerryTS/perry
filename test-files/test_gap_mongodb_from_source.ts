// The npm `mongodb` driver compiled from its real package source (the native
// perry-ext-mongodb binding was removed). CRUD against an in-process fake
// MongoDB wire-protocol server, so the whole driver stack runs: connection
// string parsing, SDAM/server selection, the OP_QUERY handshake, OP_MSG
// command encoding, BSON, cursors and write results.
import { MongoClient } from "mongodb";
import net from "node:net";
import { startFakeMongo } from "./_helpers/fake_mongo_server.ts";
import { createRequire } from "node:module";

// Print the resolved package version so the output proves what ran.
const requireFromHere = createRequire(import.meta.url);
console.log("mongodb version:", requireFromHere("mongodb/package.json").version);

const server = startFakeMongo(net, async (port: number, served: string[]) => {
  const client = new MongoClient("mongodb://127.0.0.1:" + port + "/?directConnection=true", {
    serverSelectionTimeoutMS: 5000,
    heartbeatFrequencyMS: 60000,
  });
  try {
    await client.connect();
    console.log("connected");
    const collection = client.db("gap").collection("documents");
    const ins = await collection.insertMany([{ a: 3 }, { a: 1 }, { a: 2 }]);
    console.log("insertMany", ins.acknowledged, ins.insertedCount, Object.keys(ins.insertedIds).length);
    const sorted = await collection.find({}).sort({ a: 1 }).toArray();
    console.log("find sorted", sorted.map((d: any) => d.a).join(","));
    const one = await collection.findOne({ a: 2 });
    console.log("findOne", one ? one.a : null, one ? typeof one._id.toHexString() : null);
    const upd = await collection.updateOne({ a: 3 }, { $set: { b: "x" } });
    console.log("updateOne", upd.matchedCount, upd.modifiedCount);
    const cnt = await collection.countDocuments({ a: { $gte: 2 } });
    console.log("countDocuments", cnt);
    const del = await collection.deleteMany({ a: { $lt: 3 } });
    console.log("deleteMany", del.deletedCount);
    const rest = await collection.find({}).toArray();
    console.log("remaining", rest.map((d: any) => d.a + ":" + d.b).join(","));
    console.log("served", served.join(","));
  } catch (e: any) {
    console.log("ERR", e && e.name, e && e.message);
  } finally {
    await client.close();
    server.close();
  }
});
