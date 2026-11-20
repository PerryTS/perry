async function noThrow(i: number): Promise<number> {
  try { await Promise.resolve(i); return i; } catch { return -1; }
}
async function withThrow(i: number): Promise<number> {
  try { await Promise.reject(new Error("boom")); return i; } catch { return -1; }
}
let finallyRuns = 0;
async function withFinally(i: number): Promise<number> {
  try { await Promise.resolve(i); return i; } finally { finallyRuns++; }
}
async function nested(i: number): Promise<number> {
  try {
    await Promise.resolve(i);
    try { await Promise.reject(new Error("inner")); return 999; } catch { return i; }
  } catch { return -2; }
}
async function main(): Promise<void> {
  let sumNoThrow = 0;
  for (let i = 0; i < 400; i++) sumNoThrow += await noThrow(i);
  console.log("noThrow sum=" + sumNoThrow);
  let sumThrow = 0;
  for (let i = 0; i < 400; i++) sumThrow += await withThrow(i);
  console.log("withThrow sum=" + sumThrow);
  let sumFinally = 0;
  for (let i = 0; i < 400; i++) sumFinally += await withFinally(i);
  console.log("withFinally sum=" + sumFinally + " finallyRuns=" + finallyRuns);
  let sumNested = 0;
  for (let i = 0; i < 400; i++) sumNested += await nested(i);
  console.log("nested sum=" + sumNested);
  console.log("DONE");
}
main().catch((e) => console.log("ERR: " + String((e as any)?.message ?? e)));
