import { WASI } from "node:wasi";
const Constructor = WASI;
const wasi = new Constructor({ version: "preview1" });
console.log(typeof wasi.wasiImport.args_get);
class Guest extends WASI {}
const guest = new Guest({ version: "preview1" });
console.log(guest instanceof WASI, typeof guest.getImportObject().wasi_snapshot_preview1.args_get);
