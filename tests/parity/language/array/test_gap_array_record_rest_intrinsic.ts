const source: any = [1,2,3];
const push = Array.prototype.push;
let calls = 0;
try { Array.prototype.push = function(...xs: any[]): any { calls++; throw 1; }; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
const [first,...rest] = source;
try { Array.prototype.push = push; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
console.log("push-override", calls, first, rest[0], rest[1], rest.length);
let setters = 0;
try { Object.defineProperty(Array.prototype, "1", {get() { return 99; }, set(v) { setters++; }, configurable:true}); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
const [...all] = source;
delete (Array.prototype as any)[1];
console.log("dense-own", setters, all[0], all[1], all[2], 1 in all);
