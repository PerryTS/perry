// SNC 6: a rejection after N fused resumes: catch / finally / rethrow ordering.
const log: string[] = [];
async function f(): Promise<string> {
  let i = 0;
  try {
    for (; i < 6; i++) await i;
    log.push("before-reject");
    await Promise.reject(new Error("boom" + i));
    log.push("unreachable");
  } catch (e: any) {
    log.push("caught " + e.message);
    await 0;
    log.push("after-catch-await");
  } finally {
    log.push("finally");
    await 1;
    log.push("finally-after-await");
  }
  for (let k = 0; k < 4; k++) await k;
  return "ok";
}
async function g(): Promise<void> {
  for (let i = 0; i < 5; i++) await i;
  throw new Error("escaped");
}
f().then((v) => log.push("f " + v));
g().catch((e) => log.push("g " + e.message));
Promise.resolve().then(() => log.push("p1")).then(() => log.push("p2")).then(() => log.push("p3"));
setTimeout(() => console.log(log.join("\n")), 5);
