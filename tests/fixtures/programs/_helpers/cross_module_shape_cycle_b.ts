// Import cycle, half B: constructs and stores into instances of A's class.
import { CycNode, linkCyc } from "./cross_module_shape_cycle_a.ts";

export function buildFromB(count: number): CycNode[] {
  const out: CycNode[] = [];
  for (let i = 0; i < count; i++) out.push(new CycNode(i));
  return out;
}

export function relinkInB(nodes: CycNode[], rounds: number): number {
  let sum = 0;
  for (let r = 0; r < rounds; r++) {
    for (let i = 0; i < nodes.length; i++) {
      nodes[i].next = nodes[(i + r) % nodes.length];
      nodes[i].value += 1;
      sum += nodes[i].next!.value;
    }
    linkCyc(nodes[0], null);
  }
  return sum;
}
