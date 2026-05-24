import { duplexPair } from "node:stream";
// duplexPair() returns two paired Duplex streams: data written to one
// appears as readable data on the other (bidirectional in-memory pipe).
const [a, b] = (duplexPair as any)();
const chunks: string[] = [];
b.on("data", (c: any) => chunks.push(String(c)));
b.on("end", () => console.log("b got:", chunks.join("|")));
a.write("hello ");
a.write("world");
a.end();
