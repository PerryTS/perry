function consume(a: any, action: number) {
  const trace: string[] = [];
  try {
    for (const x of a) {
      trace.push("body" + x);
      try {
        if (action === 1) break;
        if (action === 2) throw "body-error";
        if (action === 3) return trace.join("|");
        if (action === 4) continue;
      } finally { trace.push("finally"); }
    }
  } catch (e) { trace.push("caught:" + e); }
  return trace.join("|");
}
const values: any = Array.prototype[Symbol.iterator];
const aip: any = Object.getPrototypeOf(values.call([]));
aip.return = function(this: any) {
  console.log("array close", this.next().value);
  return { done: true };
};
for (let i = 0; i < 5; i++) console.log("array", i, consume([1, 2], i));
delete aip.return;
function custom(failStep: boolean, failClose: boolean): any {
  return { [Symbol.iterator]() {
    let i = 0;
    return {
      next() { if (failStep) throw "step-error"; return i < 2 ? {value: ++i, done: false} : {done:true}; },
      return() { console.log("protocol close"); if (failClose) throw "close-error"; return {done:true}; }
    };
  }};
}
for (let i = 0; i < 5; i++) console.log("custom", i, consume(custom(false, false), i));
console.log("step", consume(custom(true, false), 2));
console.log("throw-close", consume(custom(false, true), 2));
console.log("break-close", consume(custom(false, true), 1));
const a: any = [1, 2];
Object.defineProperty(a, "0", { get() { throw "get-error"; } });
aip.return = function() { console.log("unexpected close"); return { done:true }; };
console.log("get", consume(a, 2));
delete aip.return;
