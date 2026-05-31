import { Utf8Stream } from "node:fs";
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_utf8stream_reopen_destroy";
try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

function waitClose(stream: any) {
  return new Promise<void>((resolve) => {
    stream.on("close", () => resolve());
  });
}

const listenerStream = new Utf8Stream({ dest: ROOT + "/listeners.txt", sync: true });
let calls = 0;
function handler() {
  calls++;
}
listenerStream.on("custom", handler);
listenerStream.once("custom", () => {
  calls += 10;
});
console.log("utf8stream listener count one:", listenerStream.listenerCount("custom"));
console.log("utf8stream emit custom:", listenerStream.emit("custom"));
console.log("utf8stream listener count two:", listenerStream.listenerCount("custom"), calls);
listenerStream.off("custom", handler);
console.log("utf8stream listener count off:", listenerStream.listenerCount("custom"));
listenerStream.addListener("custom", handler);
listenerStream.removeListener("custom", handler);
console.log("utf8stream listener count remove:", listenerStream.listenerCount("custom"));
listenerStream.addListener("custom", handler);
listenerStream.removeAllListeners("custom");
console.log("utf8stream listener count all:", listenerStream.listenerCount("custom"));
const listenerDestroyReturn = listenerStream.destroy();
console.log("utf8stream listener destroy return:", listenerDestroyReturn === undefined);

const firstPath = ROOT + "/first.txt";
const secondPath = ROOT + "/second.txt";
const reopenStream = new Utf8Stream({ dest: firstPath, sync: true, minLength: 1, maxWrite: 8 });
reopenStream.write("one");
const reopenReturn = reopenStream.reopen(secondPath);
console.log("utf8stream reopen return:", reopenReturn === undefined);
reopenStream.write("two");
const reopenClosed = waitClose(reopenStream);
const endReturn = reopenStream.end();
console.log("utf8stream end return:", endReturn === undefined);
await reopenClosed;
console.log("utf8stream reopen first:", fs.readFileSync(firstPath, "utf8"));
console.log("utf8stream reopen second:", fs.readFileSync(secondPath, "utf8"));

const destroyPath = ROOT + "/destroy.txt";
const destroyStream = new Utf8Stream({ dest: destroyPath, sync: true, minLength: 5, maxWrite: 6 });
destroyStream.write("ab");
const destroyClosed = waitClose(destroyStream);
const destroyReturn = destroyStream.destroy();
console.log("utf8stream destroy return:", destroyReturn === undefined);
await destroyClosed;
console.log("utf8stream destroy drops buffer:", fs.readFileSync(destroyPath, "utf8"));

const disposePath = ROOT + "/dispose.txt";
const disposeStream = new Utf8Stream({ dest: disposePath, sync: true, minLength: 5, maxWrite: 6 });
disposeStream.write("zz");
const disposeClosed = waitClose(disposeStream);
const disposeReturn = disposeStream[Symbol.dispose]();
console.log("utf8stream dispose return:", disposeReturn === undefined);
await disposeClosed;
console.log("utf8stream dispose drops buffer:", fs.readFileSync(disposePath, "utf8"));
