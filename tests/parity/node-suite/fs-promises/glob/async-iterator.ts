(process as any).emitWarning = () => {};
import * as fs from "node:fs";
import { glob } from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_glob_async_iterator";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT + "/a", { recursive: true });
fs.mkdirSync(ROOT + "/dist", { recursive: true });
fs.writeFileSync(ROOT + "/top.txt", "top");
fs.writeFileSync(ROOT + "/a/nested.txt", "nested");
fs.writeFileSync(ROOT + "/a/x.js", "x");
fs.writeFileSync(ROOT + "/a/y.ts", "y");
fs.writeFileSync(ROOT + "/dist/out.js", "out");

const list = (value: string[]) => value.slice().sort().join(",");

const manual = glob("top.txt", { cwd: ROOT }) as any;
console.log("glob iterator self:", String(manual[Symbol.asyncIterator]() === manual));
console.log("glob iterator methods:", `${typeof manual.next}:${typeof manual.return}`);
const first = await manual.next();
console.log("glob manual next:", JSON.stringify({ value: first.value, done: first.done }));
const returned = await manual.return();
console.log("glob return:", JSON.stringify({ done: returned.done, value: returned.value ?? null }));
const afterReturn = await manual.next();
console.log("glob after return:", JSON.stringify({ done: afterReturn.done, value: afterReturn.value ?? null }));

const txtMatches: string[] = [];
for await (const entry of glob("**/*.txt", { cwd: ROOT })) {
  txtMatches.push(entry);
}
console.log("glob for await:", list(txtMatches));

const advanced: string[] = [];
for await (const entry of glob("**/*.{js,ts}", { cwd: ROOT, exclude: ["dist/**"] })) {
  advanced.push(entry);
}
console.log("glob advanced exclude:", list(advanced));

const dirents: string[] = [];
for await (const entry of glob("a/*.js", { cwd: ROOT, withFileTypes: true }) as AsyncIterable<fs.Dirent>) {
  dirents.push(`${entry.name}:${entry.isFile()}:${entry.isDirectory()}:${typeof (entry as any).parentPath}`);
}
console.log("glob dirent:", dirents.join(","));

async function nextCode(pattern: any, options?: any): Promise<string> {
  try {
    await (glob(pattern, options) as any).next();
    return "ok";
  } catch (err) {
    return (err as any).code || (err as Error).name;
  }
}

console.log("glob invalid pattern:", await nextCode(42));
console.log("glob invalid options:", await nextCode("*", null));
