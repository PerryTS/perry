// #4916: v8.writeHeapSnapshot must emit a REAL object graph (V8
// snapshot format), not an empty-but-valid document. Output is
// boolean-only so it byte-matches `node --experimental-strip-types`,
// which dumps its own (different-sized) real graph.
import v8 from "node:v8";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

// Hold a recognizable graph alive so the dump must contain it.
const heapMarkerArray = ["__heap_marker_string_4916__", { heapMarkerKey4916: 1 }];

const file = path.join(os.tmpdir(), `perry-heapsnap-${process.pid}.heapsnapshot`);
const written = v8.writeHeapSnapshot(file);
console.log("returns path:", written === file);

const snap = JSON.parse(fs.readFileSync(file, "utf8"));
console.log(
  "has snapshot meta:",
  typeof snap.snapshot === "object" && typeof snap.snapshot.meta === "object",
);
console.log("node_fields:", JSON.stringify(snap.snapshot.meta.node_fields));
console.log("edge_fields:", JSON.stringify(snap.snapshot.meta.edge_fields));

const nodeCount = snap.snapshot.node_count;
const edgeCount = snap.snapshot.edge_count;
console.log("node_count > 0:", nodeCount > 0);
console.log("edge_count > 0:", edgeCount > 0);
const nodeFieldCount = snap.snapshot.meta.node_fields.length;
console.log("nodes len matches:", snap.nodes.length === nodeCount * nodeFieldCount);
console.log("edges len matches:", snap.edges.length === edgeCount * 3);
console.log(
  "strings non-empty:",
  Array.isArray(snap.strings) && snap.strings.length > 0,
);
console.log(
  "marker string present:",
  snap.strings.includes("__heap_marker_string_4916__"),
);
console.log("marker key present:", snap.strings.includes("heapMarkerKey4916"));
console.log("marker alive:", heapMarkerArray.length === 2);

fs.unlinkSync(file);
