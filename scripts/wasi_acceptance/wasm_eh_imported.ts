import { thrower } from "./_helpers/wasm_eh_imported.ts";
try { thrower.get({}, "direct"); } catch (e: any) { console.log(e.message); }
const get = thrower.get;
try { get({}, "indirect"); } catch (e: any) { console.log(e.message); }
console.log("imported continued");
