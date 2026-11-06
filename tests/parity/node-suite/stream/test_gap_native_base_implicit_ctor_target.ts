// `class Y extends X {}` where X extends a native base: Y's implicit
// constructor runs X's with new.target = Y, so the instance's prototype is
// Y.prototype for every native base (explicit and implicit constructors).
import { Readable } from "node:stream";
import { EventEmitter } from "node:events";

const P = (o: any, C: any) => Object.getPrototypeOf(o) === C.prototype;
let seen: any;

class WX extends WeakMap<object, number> { constructor(e?: any) { super(e); seen = new.target; } }
class WY extends WX {}
class WZ extends WX { constructor(e?: any) { super(e); } }
const k = {};
const wy = new WY([[k, 3]]);
console.log("weakmap", P(wy, WY), seen === WY, wy instanceof WX, wy instanceof WeakMap, wy.get(k), P(new WZ(), WZ));

class SX extends WeakSet<object> { constructor(e?: any) { super(e); seen = new.target; } }
class SY extends SX {}
const sy = new SY([k]);
console.log("weakset", P(sy, SY), seen === SY, sy.has(k));

class MX extends Map { constructor(e?: any) { super(e); seen = new.target; } }
class MY extends MX {}
const my = new MY([[1, 2]]);
console.log("map", P(my, MY), seen === MY, my.get(1));

class TX extends Set { constructor(e?: any) { super(e); seen = new.target; } }
class TY extends TX {}
const ty = new TY([5]);
console.log("set", P(ty, TY), seen === TY, ty.has(5));

class AX extends Array { constructor(...a: any[]) { super(...a); seen = new.target; } }
class AY extends AX {}
const ay = new AY(1, 2, 3);
console.log("array", P(ay, AY), seen === AY, ay.length);

class EX extends Error { constructor(m?: string) { super(m); seen = new.target; } }
class EY extends EX {}
const ey = new EY("m");
console.log("error", P(ey, EY), seen === EY, ey.message);

class RX extends Readable { constructor(o?: any) { super(o); seen = new.target; } }
class RY extends RX {}
const ry = new RY({ read() {} });
console.log("readable", P(ry, RY), seen === RY, ry instanceof Readable);

class EEX extends EventEmitter { constructor() { super(); seen = new.target; } }
class EEY extends EEX {}
console.log("ee", P(new EEY(), EEY), seen === EEY);

// Two implicit levels.
class W3 extends WY {}
console.log("weakmap-2", P(new W3(), W3), seen === W3);
