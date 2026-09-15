// Module B for test_gap_cross_module_class_shape_identity.ts: a NON-entry
// module that imports the class and constructs its instances, both at module
// init and on demand.
import { Node6, Sub6 } from "./cross_module_class_shape_identity.ts";

export const builtAtInit: Node6[] = [new Node6(100), new Node6(101)];

export function buildChain(count: number): Node6 {
  let head = new Node6(0);
  for (let i = 1; i < count; i++) {
    const n = i % 5 === 0 ? new Sub6(i) : new Node6(i);
    n.next = head;
    head = n;
  }
  return head;
}

export function buildPair(v: number): Node6[] {
  return [new Node6(v), new Node6(v + 1)];
}
