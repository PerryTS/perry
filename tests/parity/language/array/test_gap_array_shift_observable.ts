// #10060: shift must preserve live indexed operations and abrupt completion order.
const inherited: number[] = [1, , 3];
const proto = Object.create(Array.prototype);
proto[1] = 8;
Object.setPrototypeOf(inherited, proto);
console.log("inherited", inherited.shift(), inherited[0], inherited[1], inherited.length);

const observed: any[] = [10, 20, 30];
let log = "";
Object.defineProperty(observed, "0", {
  configurable: true,
  get() { log += "get0;"; return 10; },
  set(v) { log += "set0=" + v + ";"; },
});
Object.defineProperty(observed, "1", {
  configurable: true,
  get() { log += "get1;"; return 20; },
  set(v) { log += "set1=" + v + ";"; },
});
console.log("accessor", observed.shift(), log, observed.length);

const locked: number[] = [1, 2, 3];
Object.defineProperty(locked, "length", { writable: false });
try { locked.shift(); } catch (e) { console.log("length-error", e instanceof TypeError); }
console.log("length-effects", locked.length, locked[0], locked[1], 2 in locked);

const sealed: number[] = [1, 2, 3];
Object.seal(sealed);
try { sealed.shift(); } catch (e) { console.log("sealed-error", e instanceof TypeError); }
console.log("sealed-effects", sealed.join(","));

const frozen: any[] = [1, 2];
let reads = 0;
Object.defineProperty(frozen, "0", { get() { reads++; return 1; } });
Object.freeze(frozen);
try { frozen.shift(); } catch (e) { console.log("frozen-error", e instanceof TypeError, reads); }
const empty: number[] = [];
Object.defineProperty(empty, "length", { writable: false });
try { empty.shift(); } catch (e) { console.log("empty-error", e instanceof TypeError); }
