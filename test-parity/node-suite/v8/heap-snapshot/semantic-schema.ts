import { getHeapSnapshot } from "node:v8";

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
