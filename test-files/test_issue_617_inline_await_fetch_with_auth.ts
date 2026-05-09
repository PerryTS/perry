// Issue #617: `await fetchWithAuth(url, auth)` (and POST variant) inline
// returned `undefined` while the explicit two-step
// `let p = fetchWithAuth(...); await p` produced the resolved Response.
//
// Both forms must produce a real object response from a working server.
import { createServer } from "node:http";

const PORT = 38617;

async function main() {
  const server = createServer((req, res) => {
    res.statusCode = 200;
    res.setHeader("Content-Type", "application/json");
    res.end('{"ok":true}');
  });
  server.listen(PORT, "127.0.0.1");
  await new Promise((r) => setTimeout(r, 200));

  const url = "http://127.0.0.1:" + PORT + "/";
  const auth = "Bearer testtoken";

  // GET — inline form
  const r1 = await fetchWithAuth(url, auth);
  console.log("inline GET typeof:", typeof r1);
  console.log("inline GET status:", (r1 as any).status);

  // GET — two-step form
  const p2 = fetchWithAuth(url, auth);
  const r2 = await p2;
  console.log("two-step GET typeof:", typeof r2);
  console.log("two-step GET status:", (r2 as any).status);

  // POST — inline form
  const r3 = await fetchPostWithAuth(url, auth, '{"q":1}');
  console.log("inline POST typeof:", typeof r3);
  console.log("inline POST status:", (r3 as any).status);

  // POST — two-step form
  const p4 = fetchPostWithAuth(url, auth, '{"q":1}');
  const r4 = await p4;
  console.log("two-step POST typeof:", typeof r4);
  console.log("two-step POST status:", (r4 as any).status);

  server.close();
}

main();
