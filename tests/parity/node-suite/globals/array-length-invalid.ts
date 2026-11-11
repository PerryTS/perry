function show(label: string, value: any) {
  console.log(label + ":", String(value));
}

function showArray(label: string, value: any[]) {
  show(label + " length", value.length);
  show(label + " has0", 0 in value);
  show(label + " first", value[0]);
}

function expectThrow(label: string, fn: () => any) {
  try {
    fn();
    show(label + " result", "no throw");
  } catch (err: any) {
    show(label + " throw", err.name + " " + err.message);
  }
}

expectThrow("new neg", () => new Array(-1));
expectThrow("new frac", () => new Array(2.5));
expectThrow("new over", () => new Array(4294967296));
expectThrow("new nan", () => new Array(NaN));
expectThrow("new inf", () => new Array(Infinity));
expectThrow("call neg", () => Array(-1));
expectThrow("call frac", () => Array(2.5));
expectThrow("call over", () => Array(4294967296));
expectThrow("call nan", () => Array(NaN));
expectThrow("call inf", () => Array(Infinity));

showArray("new valid", new Array(3));
showArray("call valid", Array(2));
showArray("single string", Array("3"));
showArray("single undefined", Array(undefined));

const typed: number[] = [1, 2, 3];
expectThrow("set neg", () => {
  typed.length = -1;
});
show("set neg length", typed.length);
expectThrow("set frac", () => {
  typed.length = 2.5;
});
show("set frac length", typed.length);
expectThrow("set over", () => {
  typed.length = 4294967296;
});
show("set over length", typed.length);
expectThrow("set nan", () => {
  typed.length = NaN;
});
show("set nan length", typed.length);
expectThrow("set inf", () => {
  typed.length = Infinity;
});
show("set inf length", typed.length);
typed.length = 2;
show("set valid length", typed.length);

const dynamic: any = [1, 2, 3];
expectThrow("dynamic set neg", () => {
  dynamic.length = -1;
});
show("dynamic set neg length", dynamic.length);
expectThrow("dynamic set frac", () => {
  dynamic.length = 2.5;
});
show("dynamic set frac length", dynamic.length);
expectThrow("dynamic set over", () => {
  dynamic.length = 4294967296;
});
show("dynamic set over length", dynamic.length);
expectThrow("dynamic set nan", () => {
  dynamic.length = NaN;
});
show("dynamic set nan length", dynamic.length);
expectThrow("dynamic set inf", () => {
  dynamic.length = Infinity;
});
show("dynamic set inf length", dynamic.length);
dynamic.length = 1;
show("dynamic set valid length", dynamic.length);
