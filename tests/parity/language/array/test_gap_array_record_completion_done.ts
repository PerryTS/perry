let closes = 0;
function source(values: any[]): any {
  const array: any = values;
  array[Symbol.iterator] = function () {
    let index = 0;
    return {
      next() { return index < values.length
        ? { value: values[index++], done: false } : { done: true }; },
      return() { closes++; return { done: true }; }
    };
  };
  return array;
}
const [a] = source([10, 20]); console.log(a, closes);
const [b, c] = source([30]); console.log(b, c, closes);
const [...rest] = source([40, 50]); console.log(rest.length, closes);
function fail(): never { throw "binding"; }
try { const [d = fail()] = source([undefined]); } catch (e) { console.log(e, closes); }
for (let i = 0; i < 20; i++) {
  const [x, y] = [i, i + 1];
  if (x !== i || y !== i + 1) throw "plain";
}
console.log("ok");
