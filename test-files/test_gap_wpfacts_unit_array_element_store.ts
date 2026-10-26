// Per-unit facts (decision 151): `compactQueue` is a closed function, a
// unit of its own (decision 13). `compact` passes it an array whose
// elements hold `top`; the callee stores through `item.obj[item.prop]`, a
// computed key on what an element of its argument holds (qs
// `utils.compact`). Every element is popped: `top.o` is written and `top`
// gains `p`, which the unit of `compact` must see (DECISIONS 207).
function compactQueue(queue: any) {
  while (queue.length > 0) {
    var item = queue.pop();
    item.obj[item.prop] = "w:" + item.prop;
  }
}
function compact(value: any) {
  var top: any = { o: value };
  var queue: any = [{ obj: top, prop: "o" }, { obj: top, prop: "p" }];
  compactQueue(queue);
  return top.o + "," + top.p + "," + Object.keys(top).join("");
}
console.log(compact(5));
console.log(compact("v"));
