import { collect } from "perry/gc";

function check(ok: boolean, message: string): void {
  if (!ok) throw new Error(message);
}

class Node {
  id: number;
  text: string;
  values: number[];
  bytes: Uint8Array;
  next: any = null;
  label: () => string;
  constructor(id: number) {
    this.id = id;
    this.text = `P3 rooted Unicode 🍐 payload number ${id}`;
    this.values = [id, id + 1, id + 2];
    this.bytes = new Uint8Array([id, id + 1, id + 2]);
    this.label = () => this.text;
  }
}

async function main(): Promise<void> {
  const retained: Node[] = [];
  const indexed = new Map<number, Node>();
  const members = new Set<Node>();
  for (let id = 0; id < 24; id++) {
    const node = new Node(id);
    retained.push(node);
    indexed.set(id, node);
    members.add(node);
  }
  for (let id = 0; id < retained.length; id++) {
    retained[id].next = retained[(id + 1) % retained.length];
  }
  for (let round = 0; round < 8; round++) {
    const temporary: any[] = [];
    for (let i = 0; i < 128; i++) {
      temporary.push({ text: `temporary ${round} ${i} 🍐`, values: [i, round] });
    }
    collect();
    await Promise.resolve(round);
    for (let id = 0; id < retained.length; id++) {
      const node = retained[id];
      check(node instanceof Node && node.id === id, "class root");
      check(node.text === `P3 rooted Unicode 🍐 payload number ${id}`, "string root");
      check(node.label() === node.text, "closure capture");
      check(node.values[2] === id + 2 && node.bytes[1] === id + 1, "array roots");
      check(node.next === retained[(id + 1) % retained.length], "cyclic edge");
      check(indexed.get(id) === node && members.has(node), "collection identity");
    }
    check(temporary.length === 128, "temporary root");
  }
  await new Promise<void>(resolve => setTimeout(resolve, 5));
  collect();
  check(retained[23].label() === "P3 rooted Unicode 🍐 payload number 23", "timer continuation");
  try {
    throw new Error("P3 retained error");
  } catch (error) {
    collect();
    check((error as Error).message === "P3 retained error", "exception root");
  }
  console.log("PASS P3 guest GC: 24 cyclic class roots, Unicode, arrays, closures, Map/Set, Promise/timer and exception roots");
}

main().catch(error => { console.error(error); process.exitCode = 1; });
