// Declares a pointer-bearing class in a module that is reached ONLY through
// `await import()` (test_gap_cross_module_class_shape_identity.ts).
export class LazyNode {
  value: number;
  next: LazyNode | null;
  label: string;
  constructor(v: number) {
    this.value = v;
    this.next = null;
    this.label = "L" + v;
  }
}

export function setLazyNext(n: LazyNode, m: LazyNode | null): void {
  n.next = m;
}

export function setLazyValue(n: LazyNode, v: number): void {
  n.value = v;
}

export function describeLazy(n: LazyNode): string {
  return n.value + ":" + (n.next === null ? "null" : String(n.next.value)) + ":" + n.label;
}
