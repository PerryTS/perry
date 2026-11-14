// The stream family reached through a dynamic import of a computed specifier
// (default flags; the lane's --strict-dynamic-import would refuse it).
const name = ["node", "stream"].join(":");
import(name).then((m: any) => {
  const r = m.Readable.from(["d1", "d2"]);
  const seen: string[] = [];
  r.on("data", (c: string) => seen.push(c));
  r.on("end", () => console.log("dynamic", seen.join(","), typeof m.Transform));
});
