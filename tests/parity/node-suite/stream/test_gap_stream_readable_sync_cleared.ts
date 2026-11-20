// A Readable whose `_readableState.sync` was cleared emits a push to its 'data'
// listener at once, before `push()` returns, instead of buffering it for the
// scheduled resume. Multipart parsers (the part stream of `@fastify/busboy`'s
// Dicer) clear it on every part so a part's data lands before the next part
// starts; with the push buffered the next part's 'header' overtook it.
import { Readable } from "node:stream";

class Part extends Readable {
  _read(): void {}
}

const log: string[] = [];

function run(name: string, clearSync: boolean, pushes: Array<string | null>): Promise<void> {
  return new Promise((resolve) => {
    const part = new Part();
    let seen = "";
    part.on("data", (d: Buffer) => {
      seen += d.toString();
      log.push(`${name} data ${d.length}`);
    });
    part.on("end", () => {
      log.push(`${name} end ${seen}`);
      resolve();
    });
    if (clearSync) (part as any)._readableState.sync = false;
    for (const p of pushes) {
      part.push(p === null ? null : Buffer.from(p));
      log.push(`${name} pushed ${p === null ? "null" : p.length}`);
    }
  });
}

(async () => {
  await run("plain", false, ["3600", null]);
  await run("cleared", true, ["3600", null]);
  await run("cleared-chunks", true, ["ab", "cd", null]);
  console.log(log.join("\n"));
})();
