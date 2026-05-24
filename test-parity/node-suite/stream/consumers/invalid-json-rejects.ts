import { Readable } from "node:stream";
import { json } from "node:stream/consumers";

const promise = json(Readable.from(["{"]));
try {
  await promise;
  console.log("resolved");
} catch (e) {
  const err = e as Error & { code?: string };
  console.log("rejected name:", err.name);
  console.log("rejected code:", err.code ?? "");
}
