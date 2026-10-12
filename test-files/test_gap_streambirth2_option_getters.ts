import { Readable, Writable, Duplex, Transform, PassThrough } from "node:stream";
const values: any = {
 objectMode: false, readableObjectMode: true, writableObjectMode: false,
 highWaterMark: 32, readableHighWaterMark: 8, writableHighWaterMark: 9,
 emitClose: true, autoDestroy: true, defaultEncoding: "utf8", encoding: "utf8",
 decodeStrings: true, readable: true, writable: true, allowHalfOpen: true,
 captureRejections: false, signal: undefined,
 read() {}, write(c: any,e: any,cb: any) {cb();}, writev(c: any,cb: any){cb();},
 destroy(e: any,cb: any){cb(e);}, final(cb: any){cb();}, construct(cb: any){cb();},
 transform(c: any,e: any,cb: any){cb(null,c);}, flush(cb: any){cb();}
};
for(const C of [Readable, Writable, Duplex, Transform, PassThrough]) {
 const counts: any = {}, opts: any = {};
 for(const k of Object.keys(values)) Object.defineProperty(opts,k,{get(){counts[k]=(counts[k]||0)+1;return values[k];},enumerable:true});
 const s: any = new C(opts);
 console.log(C.name + ":" + JSON.stringify(counts));
}

for (const C of [Transform, PassThrough]) {
  const s: any = new C({highWaterMark: 0, writableHighWaterMark: 7});
  console.log(s.readableHighWaterMark + ":" + s.writableHighWaterMark);
}
