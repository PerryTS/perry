import { duplexPair } from "node:stream";

const [a, b] = (duplexPair as any)();
const chunks: string[] = [];

a.on("data", (c: any) => chunks.push(String(c)));
a.on("end", () => console.log("a got:", chunks.join("|")));

b.write("hello ");
b.write("world");
b.end();
