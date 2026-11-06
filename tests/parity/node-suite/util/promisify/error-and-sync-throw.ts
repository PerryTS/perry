import { promisify } from "node:util";

function errback(cb: Function) { cb(new Error("bad")); }
function syncThrow(cb: Function) { throw new Error("sync"); }
try { await promisify(errback)(); console.log("errback no reject"); } catch (err: any) { console.log("errback:", err.message); }
try { await promisify(syncThrow)(); console.log("sync no throw"); } catch (err: any) { console.log("sync:", err.message); }
