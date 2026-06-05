function show(label: string, value: any) {
  console.log(label + ":", String(value));
}

function showArray(label: string, value: any[]) {
  console.log(
    label + ":",
    JSON.stringify(value),
    "length=" + value.length,
    "keys=" + Object.keys(value).join("|"),
    "has0=" + String(0 in value),
    "has1=" + String(1 in value),
    "has2=" + String(2 in value),
  );
}

const sparseLike: any = { 0: "a", 2: "c", length: 3 };
const holeOnly: any = { length: 2 };

show("join sparse object", Array.prototype.join.call(sparseLike, "|"));
show("includes missing undefined", Array.prototype.includes.call(holeOnly, undefined));
show("indexOf missing undefined", Array.prototype.indexOf.call(holeOnly, undefined));
show("indexOf object c", Array.prototype.indexOf.call(sparseLike, "c"));
show("lastIndexOf object a", Array.prototype.lastIndexOf.call(sparseLike, "a"));

showArray("slice sparse object", Array.prototype.slice.call(sparseLike, 0, 3));
showArray("map missing object", Array.prototype.map.call(holeOnly, (value: any, index: number) => {
  return String(index) + ":" + String(value);
}));

const forEachCalls: string[] = [];
Array.prototype.forEach.call(holeOnly, (value: any, index: number) => {
  forEachCalls.push(index + ":" + String(value));
});
show("forEach missing calls", forEachCalls.join("|"));

const someCalls: string[] = [];
show("some missing result", Array.prototype.some.call(holeOnly, (value: any, index: number) => {
  someCalls.push(index + ":" + String(value));
  return true;
}));
show("some missing calls", someCalls.join("|"));

show("join primitive string", Array.prototype.join.call("ab", "-"));
showArray("slice primitive string", Array.prototype.slice.call("abc", 1));
show("indexOf null throws", (() => {
  try {
    Array.prototype.indexOf.call(null as any, "x");
    return "no throw";
  } catch (err: any) {
    return err.name;
  }
})());
