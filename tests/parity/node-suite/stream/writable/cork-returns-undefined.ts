import { Writable } from "node:stream";
// cork() and uncork() return undefined (per Node docs) — they are NOT
// chainable like on().
const w = new Writable({ write(_c, _e, cb) { cb(); } });
const corkResult = w.cork();
const uncorkResult = w.uncork();
console.log("cork returns undefined:", corkResult === undefined);
console.log("uncork returns undefined:", uncorkResult === undefined);
