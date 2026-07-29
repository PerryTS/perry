import { once } from "node:events";
import { getHeapSnapshot } from "node:v8";

for (let i = 1; i <= 2; i++) {
  const stream = getHeapSnapshot();
  try {
    stream.resume();
    await once(stream, "end");
    console.log("snapshot " + i + ":", stream.readableEnded, stream.destroyed);
  } finally {
    stream.destroy();
  }
}
