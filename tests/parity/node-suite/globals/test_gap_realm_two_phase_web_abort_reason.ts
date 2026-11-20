class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
const reason = AbortSignal.abort().reason;
console.log(reason.name, reason.code, reason instanceof DOMException);
console.log(Object.getPrototypeOf(reason) === DOMException.prototype);
