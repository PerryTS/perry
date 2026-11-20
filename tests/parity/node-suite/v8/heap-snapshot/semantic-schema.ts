import { getHeapSnapshot, writeHeapSnapshot } from "node:v8";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const chunks: Buffer[] = [];
const stream = getHeapSnapshot();
for await (const chunk of stream) chunks.push(Buffer.from(chunk));
const snapshot: any = JSON.parse(Buffer.concat(chunks).toString("utf8"));
const meta = snapshot.snapshot.meta;

console.log("top keys:", Object.keys(snapshot).sort().join(","));
console.log("snapshot keys:", Object.keys(snapshot.snapshot).sort().join(","));
console.log(
  "meta arrays:",
  [meta.node_fields, meta.node_types, meta.edge_fields, meta.edge_types].every(
    Array.isArray,
  ),
);
console.log(
  "table arrays:",
  [snapshot.nodes, snapshot.edges, snapshot.strings].every(Array.isArray),
);
console.log(
  "node width:",
  meta.node_fields.length > 0,
  snapshot.nodes.length % meta.node_fields.length === 0,
);
console.log(
  "edge width:",
  meta.edge_fields.length > 0,
  snapshot.edges.length % meta.edge_fields.length === 0,
);
console.log(
  "counts:",
  snapshot.snapshot.node_count > 0,
  snapshot.snapshot.edge_count >= 0,
  snapshot.strings.length > 0,
);
console.log("stream ended:", stream.readableEnded);

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "perry-v8-schema-"));
try {
  const file = writeHeapSnapshot(path.join(dir, "schema.heapsnapshot"));
  const fileSnapshot: any = JSON.parse(fs.readFileSync(file, "utf8"));
  const fileKeys = Object.keys(fileSnapshot.snapshot).sort().join(",");
  console.log("file snapshot keys:", fileKeys);
  console.log(
    "file header matches:",
    fileKeys === Object.keys(snapshot.snapshot).sort().join(","),
    fileKeys === "edge_count,extra_native_bytes,meta,node_count,trace_function_count",
  );
} finally {
  fs.rmSync(dir, { recursive: true, force: true });
}
