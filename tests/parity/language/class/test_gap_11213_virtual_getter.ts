class Zone {
  get type(): string { return "base"; }
  get inherited(): string { return "inh:" + this.type; }
  viaMethod(): string { return "m:" + this.type; }
}
class G extends Zone { get type(): string { return "g"; } }
class H extends G { get type(): string { return "h"; } }
for (const z of [new Zone(), new G(), new H()]) {
  console.log(z.type, z.inherited, z.viaMethod());
}
const g = new G();
console.log("direct", g.type, g.inherited, g.viaMethod());
Object.defineProperty(g, "type", { value: "own", configurable: true });
console.log("own", g.type, g.inherited, g.viaMethod());
delete (g as any).type;
console.log("deleted-own", g.type, g.inherited, g.viaMethod());

class Abstract {
  get kind(): string { throw new Error("abstract"); }
  get label(): string { return "label:" + this.kind; }
  read(): string { return this.kind; }
}
class Concrete extends Abstract { get kind(): string { return "concrete"; } }
const concrete = new Concrete();
console.log("abstract", concrete.label, concrete.read());

let reads = 0;
class CountBase {
  get value(): number { return -1; }
  twice(): number { return this.value + this.value; }
}
class CountChild extends CountBase {
  get value(): number { reads++; return reads; }
}
console.log("side-effects", new CountChild().twice(), reads);

class SuperBase { get name(): string { return "parent"; } }
class SuperChild extends SuperBase {
  get name(): string { return "child:" + super.name; }
}
console.log("super", new SuperChild().name);
