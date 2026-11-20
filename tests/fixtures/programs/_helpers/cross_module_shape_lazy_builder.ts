// Statically imports the lazily reached class, and is itself reached only
// through `await import()`: it builds instances and stores into them.
import { LazyNode, setLazyNext, setLazyValue } from "./cross_module_shape_lazy_class.ts";

export const lazyAtInit: LazyNode[] = [new LazyNode(1), new LazyNode(2)];

export function buildLazy(count: number): LazyNode[] {
  const out: LazyNode[] = [];
  for (let i = 0; i < count; i++) out.push(new LazyNode(10 + i));
  return out;
}

export function relinkLazy(nodes: LazyNode[], rounds: number): number {
  let checksum = 0;
  for (let r = 0; r < rounds; r++) {
    for (let i = 0; i < nodes.length; i++) {
      const target = nodes[(i + r + 1) % nodes.length];
      if (i % 2 === 0) {
        nodes[i].next = target;
      } else {
        setLazyNext(nodes[i], target);
      }
      setLazyValue(nodes[i], nodes[i].value + 1);
      checksum += nodes[i].next!.value;
    }
  }
  return checksum;
}
