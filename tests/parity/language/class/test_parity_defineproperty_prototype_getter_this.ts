let failures = 0;
class A { _x = 42; }
Object.defineProperty(A.prototype, "px", { get() { return (this as any)._x; } });
const a: any = new A();
if (a.px !== 42) { console.log("FAIL: prototype getter this"); failures++; }

class L {
  _readableState: any;
  constructor() { this._readableState = { pipes: null }; }
}
Object.defineProperty(L.prototype, "transports", {
  configurable: false,
  enumerable: true,
  get() {
    const { pipes } = (this as any)._readableState;
    return !Array.isArray(pipes) ? [pipes].filter(Boolean) : pipes;
  },
});
const l: any = new L();
let result: any;
try { result = l.transports; } catch { failures++; }
if (JSON.stringify(result) !== "[]") failures++;
if (failures !== 0) throw new Error("defineProperty prototype getter this-binding regression failed");
console.log("defineProperty proto getter this ok");
