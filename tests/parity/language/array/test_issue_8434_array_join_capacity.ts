const show = (label: string, value: string): void => {
  console.log(`${label}:${value}|${value.length}`);
};

show("holes", new Array(3).join("|"));
show("empty", ["", "", ""].join(""));
show("unicode", ["A", "😀", "é"].join("·"));

let calls = 0;
const values: unknown[] = [];
const allocating = {
  toString(): string {
    calls++;
    for (let i = 0; i < 128; i++) {
      ("allocation-" + i).repeat(32);
    }
    values[1] = "after";
    return "before";
  },
};
values.push(allocating, "original", "tail");
show("coerce", values.join("/"));
console.log(`calls:${calls}`);
