// Import cycle, half A: declares the class. The entry imports A first, so A's
// dependency B initializes — string pool included — before A does.
import { buildFromB, relinkInB } from "./cross_module_shape_cycle_b.ts";

export class CycNode {
  value: number;
  next: CycNode | null;
  tag: string;
  constructor(v: number) {
    this.value = v;
    this.next = null;
    this.tag = "C" + v;
  }
}

export function linkCyc(n: CycNode, m: CycNode | null): void {
  n.next = m;
}

export function cycleReport(): string {
  const nodes = buildFromB(6);
  const sum = relinkInB(nodes, 20);
  for (let i = 0; i < nodes.length; i++) linkCyc(nodes[i], nodes[(i + 2) % nodes.length]);
  return sum + " " + nodes.map((n) => n.tag + ">" + (n.next === null ? "null" : n.next.tag)).join(",");
}
