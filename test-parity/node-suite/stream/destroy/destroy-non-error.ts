import { Readable } from "node:stream";
// destroy(null) / destroy(undefined) tear down without an error.
const r = new Readable({ read() {} });
let errored = false;
r.on("error", () => (errored = true));
r.on("close", () => console.log("errored:", errored, "destroyed:", r.destroyed));
r.destroy();
