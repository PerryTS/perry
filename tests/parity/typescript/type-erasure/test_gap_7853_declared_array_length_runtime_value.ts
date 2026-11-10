// #7853: an erased array annotation is not a runtime proof that the value has
// an Array layout.  The guarded inline `.length` read must preserve ordinary
// JavaScript property semantics on its fallback: missing properties are
// `undefined`, strings and array-like objects expose their real value, and a
// nullish receiver throws a catchable TypeError.

type Bag = { items: string[]; label: string };

function makeBag(value: any): Bag {
  return { items: value, label: "bag" };
}

function readLength(value: any): any {
  const bag = makeBag(value);
  const items: string[] = bag.items;
  return items.length;
}

function show(label: string, value: any): void {
  const length = readLength(value);
  console.log(label, String(length), typeof length, length === undefined);
}

show("array", ["a", "b"]);
show("number", 42);
show("string", "hi");
show("array-like number", { length: 7, 0: "z" });
show("array-like string", { length: "seven" });

function twoArgs(a: any, b: any): void {
  void a;
  void b;
}
show("function", twoArgs);
show("typed array", new Uint8Array(3));

for (const value of [null, undefined]) {
  try {
    readLength(value);
    console.log("nullish", String(value), "no throw");
  } catch (error) {
    const caught = error as Error;
    console.log(
      "nullish",
      String(value),
      caught.constructor.name + ": " + caught.message,
    );
  }
}
