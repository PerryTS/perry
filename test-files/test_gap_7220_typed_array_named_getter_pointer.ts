// A named typed-array property uses ordinary [[Get]]. Its getter may expose
// `.buffer`, which materializes backing storage and can rebind the view away
// from the codegen-cached element pointer.
let exposed: any = null;

Object.defineProperty(Uint32Array.prototype, "probe", {
  configurable: true,
  get() {
    exposed = this.buffer;
    return "getter-ran";
  },
});

const words = new Uint32Array(1);
console.log("named getter:", words["probe" as any]);
words[0] = 0x01020304;

const bytes = new Uint8Array(exposed);
console.log("shared bytes:", bytes[0], bytes[1], bytes[2], bytes[3]);
