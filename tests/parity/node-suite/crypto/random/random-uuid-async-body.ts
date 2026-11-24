// randomUUID() inside an async function body returns a fresh version-4 UUID, through a
// namespace import, a named import and the global `crypto`, before and after an await.
import * as ns from "crypto";
import { randomUUID } from "node:crypto";
const v4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
async function ids(): Promise<string> {
  const a = ns.randomUUID();
  await 0;
  const b = randomUUID();
  const c = crypto.randomUUID();
  return [typeof a, v4.test(a), v4.test(b), v4.test(c), a !== b].join(" ");
}
ids().then((v) => console.log(v));
