import { Readable } from "node:stream";
// The 'readable' event fires when data is available — alternative to
// flowing-mode 'data' events.
const r = new Readable({ read() {} });
r.on("readable", () => {
  let chunk; const out: string[] = [];
  while ((chunk = r.read())) out.push(String(chunk));
  console.log("got:", out.join(""));
});
r.push("hi");
r.push(null);
