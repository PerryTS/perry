// Test arrays containing class instances and typed objects
// Critical for static typing migration: array elements must preserve their class type

class Item {
  name: string;
  value: number;
  constructor(name: string, value: number) {
    this.name = name;
    this.value = value;
  }
  display(): string {
    return this.name + "=" + this.value;
  }
}

// === Array of class instances ===
const items: Item[] = [
  new Item("alpha", 1),
  new Item("beta", 2),
  new Item("gamma", 3),
];

console.log(items.length); // 3
console.log(items[0].name); // alpha
console.log(items[1].value); // 2
console.log(items[2].display()); // gamma=3

// === Push and access ===
items.push(new Item("delta", 4));
console.log(items.length); // 4
console.log(items[3].display()); // delta=4

// === Iterate with for loop ===
let total = 0;
for (let i = 0; i < items.length; i++) {
  total += items[i].value;
}
console.log(total); // 10

// === Map over array of objects ===
const names = items.map((item: Item): string => item.name);
console.log(names.join(", ")); // alpha, beta, gamma, delta

// === Filter array of objects ===
const big = items.filter((item: Item): boolean => item.value > 2);
console.log(big.length); // 2
console.log(big[0].name); // gamma

// === Find in array of objects ===
const found = items.find((item: Item): boolean => item.name === "beta");
if (found) {
  console.log(found.display()); // beta=2
}

// === Mutate objects through array reference ===
items[0].value = 100;
console.log(items[0].display()); // alpha=100

// === Array of objects passed to function ===
function sumValues(arr: Item[]): number {
  let s = 0;
  for (let i = 0; i < arr.length; i++) {
    s += arr[i].value;
  }
  return s;
}
console.log(sumValues(items)); // 109 (100+2+3+4)

// === Nested class instances ===
class Container {
  items: Item[];
  constructor() { this.items = []; }
  add(item: Item): void { this.items.push(item); }
  count(): number { return this.items.length; }
  getItem(index: number): Item { return this.items[index]; }
}

const box = new Container();
box.add(new Item("x", 10));
box.add(new Item("y", 20));
console.log(box.count()); // 2
console.log(box.getItem(0).display()); // x=10
console.log(box.getItem(1).display()); // y=20

// === Sort array of objects (by value) ===
const sorted = [new Item("c", 30), new Item("a", 10), new Item("b", 20)];
sorted.sort((a: Item, b: Item): number => a.value - b.value);
console.log(sorted[0].name); // a
console.log(sorted[1].name); // b
console.log(sorted[2].name); // c
