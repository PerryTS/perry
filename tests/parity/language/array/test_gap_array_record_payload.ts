// One payload must keep array identity through growth and captured iterator
// identity through user calls; neither arm may retain an obsolete input root.
function growing(a: any) {
  let sum = 0;
  for (const v of a) {
    sum += v.n;
    if (v.n === 1) for (let j = 3; j < 80; j++) a.push({n: j});
  }
  return sum;
}
function protocol() {
  let calls = 0;
  const iterator: any = {
    get next() {
      console.log("capture");
      return function() {
        calls++;
        return {value: {n: calls}, done: calls > 3};
      };
    },
    return() { console.log("close", calls); return {}; }
  };
  const input: any = [{n: 100}];
  input[Symbol.iterator] = function() { console.log("open"); return iterator; };
  let sum = 0;
  for (const v of input) {
    sum += v.n;
    if (sum === 1) {
      Object.defineProperty(iterator, "next", {value() { throw new Error("recaptured"); }});
      for (let j = 0; j < 80; j++) input.push({n:j});
    }
    if (sum >= 3) break;
  }
  return sum;
}
for (let warm = 0; warm < 4; warm++) {
  console.log("array", growing([{n:1},{n:2}]));
  console.log("protocol", protocol());
  const values: any = [{n:7},{n:8},{n:9}];
  const [first, second] = values;
  console.log("binding", first.n + second.n);
}
