import { Readable } from "node:stream";
// readable.iterator({preventCancel: true}) — but Node's option is 'destroyOnReturn'.
// The 'preventCancel' name is from Web stream's iter. Test the actual Node option.
const r = Readable.from(["a", "b", "c"]);
const it = (r as any).iterator({ destroyOnReturn: false });
const first = await it.next();
const second = await it.next();
console.log("first:", first.value);
console.log("second:", second.value);
// Early return with destroyOnReturn:false — should not destroy stream
await it.return?.();
console.log("destroyed:", r.destroyed);
