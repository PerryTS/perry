const show = (label: string, value: string): void => {
  console.log(`${label}:${value}|${value.length}`);
};

show("holes", new Array(3).join("|"));
show("empty", ["", "", ""].join(""));
show("unicode", ["A", "😀", "é"].join("·"));

let calls = 0;
const values: unknown[] = [];
const churn = (): number => {
  let n = 0;
  for (let round = 0; round < 64; round++) {
    const live: unknown[] = [];
    for (let i = 0; i < 512; i++) {
      live.push({ i, text: ("allocation-" + i).repeat(48) });
    }
    n += live.length;
  }
  return n;
};
const allocating = {
  toString(): string {
    calls++;
    churn();
    values[1] = "after";
    return "before";
  },
};
values.push(allocating, "original", "tail");
const movingSeparator = ("x" + "/").slice(1);
show("coerce", values.join(movingSeparator));
console.log(`calls:${calls}`);
