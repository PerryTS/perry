// A TypeScript AST-like declared class: optional fields are read through one
// polymorphic site while each instance has a different own-key layout.
class NodeObject {
  kind = 1;
}

const nodes: any[] = [];
for (let n = 0; n < 12; n++) {
  const node: any = new NodeObject();
  for (let k = 0; k < n; k++) node["extra" + k] = k;
  nodes.push(node);
}

function read(node: any): number {
  const value = node.optional;
  return value === undefined ? 0 : value;
}

function sumReads(): number {
  let sum = 0;
  for (let i = 0; i < 1200; i++) sum += read(nodes[i % nodes.length]);
  return sum;
}

console.log("class-absent", sumReads());

let churn: any[] = [];
for (let i = 0; i < 20000; i++) churn.push({ i });
(globalThis as any).gc();
churn = [];
console.log("after-gc", sumReads());

nodes[3].optional = 5;
console.log("own-shadow", sumReads());

(NodeObject.prototype as any).optional = 7;
console.log("prototype-add", sumReads());
(NodeObject.prototype as any).optional = 9;
console.log("prototype-value", sumReads());
delete (NodeObject.prototype as any).optional;
console.log("prototype-delete", sumReads());

(Object.prototype as any).optional = 11;
console.log("object-prototype-add", sumReads());
delete (Object.prototype as any).optional;
console.log("object-prototype-delete", sumReads());

// Two same-layout terminals exercise live intermediate-link validation.
const first = { marker: 1 };
const second = { marker: 2 };
Object.setPrototypeOf(NodeObject.prototype, first);
console.log("first-terminal", sumReads());
(first as any).optional = 13;
console.log("first-terminal-add", sumReads());
Object.setPrototypeOf(NodeObject.prototype, second);
console.log("second-terminal", sumReads());
(second as any).optional = 17;
console.log("second-terminal-add", sumReads());
