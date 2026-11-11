// A class whose parent is a built-in constructor: super() runs the realm's
// constructor, and the instance carries program objects whose sites stay
// what they are. A declared global read as a value is the realm's.
class Vec {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  add(o: Vec): Vec {
    return new Vec(this.x + o.x, this.y + o.y);
  }
}

class HttpError extends Error {
  response: Vec;
  constructor(message: string, response: Vec) {
    super(message);
    this.name = "HttpError";
    this.response = response;
  }
}

class RangeFail extends RangeError {
  at: Vec;
  constructor(at: Vec) {
    super("out of range at " + at.x);
    this.at = at;
  }
}

const a = new Vec(1, 2);
const b = new Vec(3, 4);

function load(v: Vec): string {
  try {
    throw new HttpError("status " + v.y, v);
  } catch (e) {
    const h = e as HttpError;
    return [h.name, h.message, h.response.x, e instanceof Error, e instanceof HttpError, typeof h.stack].join(" ");
  }
}

function check(v: Vec): string {
  try {
    if (v.x > 2) throw new RangeFail(v);
    return "ok " + v.x;
  } catch (e) {
    const r = e as RangeFail;
    return [r.message, r.at.y, e instanceof RangeError, e instanceof Error, Object.prototype.toString.call(e)].join(" ");
  }
}

const J: any = JSON;
const c = a.add(b);
console.log(c.x, c.y);
console.log(load(a));
console.log(load(c));
console.log(check(a));
console.log(check(c));
console.log(J.stringify({ x: c.x, y: c.y }));
console.log(String(new HttpError("plain", b)));
