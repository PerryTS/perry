import { fileURLToPath, pathToFileURL } from "node:url";

for (const u of ["file:///tmp/a%20b", "file:///tmp/%E2%82%AC", "file://host/share/file", "file:///tmp/a%2Fb"]) {
  try { console.log("fileURLToPath:", u, "=>", fileURLToPath(u)); } catch (err: any) { console.log("fileURLToPath:", u, "=>", err?.name, err?.code || "no-code"); }
}
for (const p of ["/tmp/a b", "/tmp/€", "relative/path"]) {
  try { console.log("pathToFileURL:", p, "=>", pathToFileURL(p).href); } catch (err: any) { console.log("pathToFileURL:", p, "=>", err?.name, err?.code || "no-code"); }
}
