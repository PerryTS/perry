// Module C for test_gap_cross_module_class_shape_identity.ts: a NON-entry
// module that imports the class and only stores into instances other modules
// built.
import { Node6 } from "./cross_module_class_shape_identity.ts";

export function relink(nodes: Node6[], rounds: number): number {
  let checksum = 0;
  for (let r = 0; r < rounds; r++) {
    for (let i = 0; i < nodes.length; i++) {
      const n = nodes[i];
      n.next = nodes[(i + r + 1) % nodes.length];
      n.value = n.value + 0.5;
      checksum += n.next.value;
    }
  }
  return checksum;
}
