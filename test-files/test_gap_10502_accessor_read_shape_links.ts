// The class accessor fallback is only for a bare CLASS shape link. Ordinary
// and recorded links use the existing inherited read, with the original this.
function read(receiver: any, key: string): any { return receiver[key]; }
const key = ["val", "ue"].join("");
let calls = 0;
const proto = {
  get value(): number { calls++; return this.n; },
  get absent(): undefined { calls++; return undefined; },
};

// A plain object's recorded link, including a getter returning undefined.
const plain = Object.create(proto);
plain.n = 7;
calls = 0;
console.log("plain", read(plain, key), calls);
calls = 0;
console.log("plain undefined", read(plain, "absent"), calls);

class Base {
  n: number;
  constructor(n: number) { this.n = n; }
  get value(): number { calls++; return this.n + 10; }
}
class Middle extends Base {}
class Leaf extends Middle {}
const leaf: any = new Leaf(3);
calls = 0;
console.log("class", read(leaf, key), read(leaf, key), calls);

// A recorded receiver link replaces its class's declaration prototype.
Object.setPrototypeOf(leaf, proto);
calls = 0;
console.log("relinked", read(leaf, key), read(leaf, key), calls);
calls = 0;
console.log("relinked undefined", read(leaf, "absent"), calls);

// The keyless branch must use the same recorded-link rule.
class Empty {}
const empty: any = new Empty();
Object.setPrototypeOf(empty, proto);
calls = 0;
console.log("keyless undefined", read(empty, "absent"), calls);

// A data lane must shadow an accessor, and its deletion must expose it again.
const data: any = new Leaf(4);
Object.defineProperty(Middle.prototype, "value", { value: 29, configurable: true });
calls = 0;
console.log("data", read(data, key), read(data, key), calls);
delete (Middle.prototype as any).value;
calls = 0;
console.log("exposed", read(data, key), read(data, key), calls);
