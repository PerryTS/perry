// #8079: ordinary parameter annotations are specialization candidates, never
// proofs. Direct calls may enter a proof-bearing clone only after validating
// the live argument; annotation lies, escaped/indirect calls, and accessors
// must retain the public boxed semantics.

function arrayTotal(values: number[]): any {
  let total: any = values[0];
  for (let i = 1; i < values.length; i++) {
    total = total + values[i];
  }
  return total;
}

console.log("array-good", arrayTotal([1, 2, 3, 4]));
console.log("array-wrong-elements", arrayTotal(["x", 2, 3] as any));
console.log("array-wrong-container", arrayTotal({ 0: "o", 1: 7, length: 2 } as any));

const escapedArrayTotal = arrayTotal;
console.log("array-indirect", escapedArrayTotal(["i", 5] as any));

function choose(flag: boolean, values: number[]): number {
  let selected = values[1];
  for (let i = 0; i < 1; i++) {
    if (flag) selected = values[0];
  }
  return selected;
}

console.log("scalar-good", choose(true, [11, 22]));
console.log("scalar-wrong", choose(7 as any, [11, 22]));

function addFirst(value: number, values: number[]): any {
  let result: any = value;
  for (let i = 0; i < 1; i++) result = result + values[0];
  return result;
}

// With no raw-representation call-site fact for the first argument, its live
// string value must fail the ordinary `number` descriptor and use the boxed
// body (`"n" + 2`, not a numeric coercion).
console.log("number-wrong", addFirst("n" as any, [2]));

interface Payload {
  label: string;
  count: number;
}

function render(payload: Payload): any {
  let result: any = payload.label;
  for (let i = 0; i < 1; i++) {
    result = result + ":" + payload.count;
  }
  return result;
}

console.log("object-good", render({ label: "items", count: 3 }));
console.log("object-wrong", render({ label: 9, count: "many" } as any));

let getterHits = 0;
const accessorPayload: any = { count: 4 };
Object.defineProperty(accessorPayload, "label", {
  enumerable: true,
  get() {
    getterHits++;
    return "getter";
  },
});
console.log("object-accessor", render(accessorPayload), getterHits);

function mutatePayload(payload: Payload): any {
  (payload as any).count = "changed";
  return payload.count + 1;
}

// A parameter whose reachable value is mutated is deliberately ineligible:
// an entry guard cannot prove facts that the body itself later invalidates.
console.log("object-mutated", mutatePayload({ label: "x", count: 1 }));

type Tree =
  | { kind: "leaf"; value: number }
  | { kind: "branch"; left: Tree; right: Tree };

function treeTotal(tree: Tree): any {
  if (tree.kind === "leaf") return tree.value;
  return treeTotal(tree.left) + treeTotal(tree.right);
}

const tree: Tree = {
  kind: "branch",
  left: { kind: "leaf", value: 4 },
  right: {
    kind: "branch",
    left: { kind: "leaf", value: 5 },
    right: { kind: "leaf", value: 6 },
  },
};
console.log("union-recursive", treeTotal(tree));
console.log("union-wrong", treeTotal({ kind: "leaf", value: "bad" } as any));
console.log(
  "union-nested-wrong",
  treeTotal({
    kind: "branch",
    left: { kind: "leaf", value: "bad" },
    right: { kind: "leaf", value: 1 },
  } as any),
);

class NumericBox {
  value: number;
  constructor(value: number) {
    this.value = value;
  }
}

class StringBox {
  value: string;
  constructor(value: string) {
    this.value = value;
  }
}

function bumpBox(box: NumericBox): any {
  let result: any = box.value;
  for (let i = 0; i < 1; i++) result = result + 1;
  return result;
}

console.log("class-good", bumpBox(new NumericBox(8)));
console.log("class-wrong", bumpBox(new StringBox("s") as any));

// Keep the guarded parameter live across enough allocations for the moving-GC
// matrix. Both the clone and a failed-guard fallback execute this body.
function surviveMovingGc(payload: Payload): string {
  const before = payload.label;
  const junk: any[] = [];
  for (let i = 0; i < 50000; i++) {
    junk.push({ i, text: "j" + i });
  }
  return before + ":" + payload.label + ":" + junk.length;
}

console.log("moving-clone", surviveMovingGc({ label: "live", count: 1 }));
console.log("moving-fallback", surviveMovingGc({ label: 9, count: "lie" } as any));
