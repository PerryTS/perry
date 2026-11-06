// Thread census while pool-backed bcrypt/argon2 work is in flight, with thread
// NAMES so the census says which pool ran it rather than only how many threads
// existed.
import { readdirSync, readFileSync } from "node:fs";
import bcrypt from "bcrypt";
import argon2 from "argon2";

function names(): string {
  try {
    const out: string[] = [];
    for (const t of readdirSync("/proc/self/task")) {
      try {
        out.push(readFileSync(`/proc/self/task/${t}/comm`, "utf8").trim());
      } catch {}
    }
    out.sort();
    const counts = new Map<string, number>();
    for (const n of out) counts.set(n, (counts.get(n) ?? 0) + 1);
    return [...counts.entries()].map(([n, c]) => `${n} x${c}`).join(", ");
  } catch {
    return "unavailable";
  }
}

function count(): number {
  try {
    return readdirSync("/proc/self/task").length;
  } catch {
    return -1;
  }
}

async function main(): Promise<void> {
  console.log("idle threads:", count(), "|", names());
  const inflight: Promise<unknown>[] = [];
  for (let i = 0; i < 8; i++) inflight.push(bcrypt.hash("password" + i, 11));
  for (let i = 0; i < 4; i++) inflight.push(argon2.hash("password" + i));
  await new Promise<void>((r) => setTimeout(r, 200));
  console.log("in-flight threads:", count(), "|", names());
  const results = await Promise.all(inflight);
  console.log("hashes produced:", results.length);
  console.log(
    "all look like hashes:",
    results.every((r) => typeof r === "string" && (r as string).length > 20),
  );
  const ok = await bcrypt.compare("password0", results[0] as string);
  const bad = await bcrypt.compare("wrong", results[0] as string);
  console.log("bcrypt verify correct:", ok, "wrong:", bad);
  const aok = await argon2.verify(results[8] as string, "password0");
  console.log("argon2 verify correct:", aok);
  console.log("after threads:", count(), "|", names());
}

main();
