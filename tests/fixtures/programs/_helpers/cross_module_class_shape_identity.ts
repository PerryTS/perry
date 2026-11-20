// Producer module for test_gap_cross_module_class_shape_identity.ts.
// Node6 has number, pointer and string fields, so its defining module mints a
// typed ShapeId; instances allocated by the importer must share that identity.
export class Node6 {
  value: number;
  next: Node6 | null;
  label: string;
  constructor(v: number) {
    this.value = v;
    this.next = null;
    this.label = "n" + v;
  }
  link(m: Node6 | null): void {
    this.next = m;
  }
  bump(by: number): void {
    this.value = this.value + by;
  }
  total(): number {
    let sum = 0;
    let cursor: Node6 | null = this;
    while (cursor !== null) {
      sum += cursor.value;
      cursor = cursor.next;
    }
    return sum;
  }
}

export class Sub6 extends Node6 {
  extra: number;
  constructor(v: number) {
    super(v);
    this.extra = v * 2;
  }
}

export class Empty6 {}

export const Expr6 = class {
  a: number;
  b: Node6 | null;
  constructor(a: number) {
    this.a = a;
    this.b = null;
  }
};

export function makeNode(v: number): Node6 {
  return new Node6(v);
}

export function setNext(n: Node6, m: Node6 | null): void {
  n.next = m;
}

export function setValue(n: Node6, v: number): void {
  n.value = v;
}

export function describe(n: Node6): string {
  return n.value + ":" + (n.next === null ? "null" : String(n.next.value)) + ":" + n.label;
}
