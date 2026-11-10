// #11891: a Proxy passed through a `T[]` parameter must keep Proxy [[Set]]
// semantics. A declared array element type is a hint, never proof that the
// receiver has an ArrayHeader layout.

class Body {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}

function fillNumbers(xs: number[]): void {
  for (let i = 0; i < xs.length; i++) xs[i] = i * 10;
}
function setNumber(xs: number[]): void {
  xs[1] = 77;
}
function fillBodies(xs: Body[]): void {
  for (let i = 0; i < xs.length; i++) xs[i] = new Body(i + 10);
}
function setBody(xs: Body[]): void {
  xs[1] = new Body(99);
}
function fillStrings(xs: string[]): void {
  for (let i = 0; i < xs.length; i++) xs[i] = `loop-${i}`;
}
function setString(xs: string[]): void {
  xs[1] = "straight";
}

const numberTarget = [1, 2, 3];
const numberKeys: string[] = [];
const numberProxy = new Proxy(numberTarget, {
  set(target: any, key: any, value: any) {
    numberKeys.push(String(key));
    target[key] = value;
    return true;
  },
});
fillNumbers(numberProxy);
setNumber(numberProxy);
console.log("number", numberTarget.join(","), numberKeys.join(","));

const bodyTarget = [new Body(1), new Body(2)];
const bodyKeys: string[] = [];
const bodyProxy = new Proxy(bodyTarget, {
  set(target: any, key: any, value: any) {
    bodyKeys.push(String(key));
    target[key] = value;
    return true;
  },
});
fillBodies(bodyProxy);
setBody(bodyProxy);
console.log("body", bodyTarget[0].x, bodyTarget[1].x, bodyKeys.join(","));

const stringTarget = ["a", "b", "c"];
const stringKeys: string[] = [];
const stringProxy = new Proxy(stringTarget, {
  set(target: any, key: any, value: any) {
    stringKeys.push(String(key));
    target[key] = value;
    return true;
  },
});
fillStrings(stringProxy);
setString(stringProxy);
console.log("string", stringTarget.join(","), stringKeys.join(","));

const rejectedTarget = [5];
try {
  setNumber(new Proxy(rejectedTarget, { set() { return false; } }));
  console.log("reject missed");
} catch (error: any) {
  console.log("reject", error.name, rejectedTarget[0]);
}
