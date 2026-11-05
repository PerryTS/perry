// Issue #652: class methods from precompiled .js (node_modules) drop on instance.
// Pre-fix `typeof p.query` was `undefined` for any class imported from a .js
// file that perry's CJS-wrap pipeline routes through the IIFE shape. The fix
// hoists top-level class declarations OUT of the IIFE so the consumer's
// `import { Pool }` resolves to the actual class, not `_cjs.Pool`.
import { Base, Pool } from "minilib";

async function main() {
  console.log("typeof Base:", typeof Base);
  console.log("typeof Pool:", typeof Pool);
  const b = new Base("alice");
  console.log("base greet:", b.greet());
  const p = new Pool({ url: "x://", name: "main" });
  console.log("typeof p.query:", typeof (p as any).query);
  console.log("pool name:", p.name);
  console.log("pool greet:", p.greet());
  console.log("pool result:", JSON.stringify(await p.query("Q")));
}
main().catch((e) => { console.error(e); process.exit(2); });
