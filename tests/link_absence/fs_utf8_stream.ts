// HWS companion: fs
// HWS hook: js_nm_install_fs
// HWS prefixes: perry_runtime::fs::|js_fs_
import {Utf8Stream,readFileSync,writeFileSync} from "node:fs";
import {EventEmitter} from "node:events";
const path="hws-utf8.txt";writeFileSync(path,"");
const stream=new Utf8Stream({dest:path,sync:true});
EventEmitter.prototype.on.call(stream,"hws",value=>console.log(value));
EventEmitter.prototype.emit.call(stream,"hws",42);
EventEmitter.prototype.removeAllListeners.call(stream,"hws");
console.log(EventEmitter.prototype.listenerCount.call(stream,"hws"));
stream.write("payload");stream.flushSync();console.log(readFileSync(path,"utf8").trim());stream.destroy();

// This positive witness also exercises the complete module installer.
import * as moduleSurface from "node:fs";
if (Object.keys(moduleSurface).length === 0) throw new Error("empty builtin namespace");
