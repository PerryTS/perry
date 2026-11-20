// Gap test: typeof and instanceof operations
// Run: node --experimental-strip-types test_gap_typeof_instanceof.ts

// --- typeof for every type ---
console.log("typeof undefined:", typeof undefined);
console.log("typeof null:", typeof null);
console.log("typeof true:", typeof true);
console.log("typeof false:", typeof false);
console.log("typeof 0:", typeof 0);
console.log("typeof 42:", typeof 42);
console.log("typeof 3.14:", typeof 3.14);
console.log("typeof NaN:", typeof NaN);
console.log("typeof Infinity:", typeof Infinity);
console.log("typeof -Infinity:", typeof -Infinity);
console.log("typeof string:", typeof "hello");
console.log("typeof empty string:", typeof "");
console.log("typeof object:", typeof {});
console.log("typeof array:", typeof []);
console.log("typeof function:", typeof (() => {}));
console.log("typeof bigint:", typeof BigInt(42));

// --- typeof in conditions ---
function checkType(x: any): string {
  if (typeof x === "number") return "number";
  if (typeof x === "string") return "string";
  if (typeof x === "boolean") return "boolean";
  if (typeof x === "undefined") return "undefined";
  if (typeof x === "object" && x === null) return "null";
  if (typeof x === "object") return "object";
  if (typeof x === "function") return "function";
  return "unknown";
}

console.log("check 42:", checkType(42));
console.log("check hello:", checkType("hello"));
console.log("check true:", checkType(true));
console.log("check undefined:", checkType(undefined));
console.log("check null:", checkType(null));
console.log("check {}:", checkType({}));
console.log("check []:", checkType([]));
console.log("check fn:", checkType(() => {}));

// --- instanceof with classes ---
class Animal {
  name: string;
  constructor(name: string) { this.name = name; }
}

class Dog extends Animal {
  breed: string;
  constructor(name: string, breed: string) {
    super(name);
    this.breed = breed;
  }
}

class Cat extends Animal {
  indoor: boolean;
  constructor(name: string, indoor: boolean) {
    super(name);
    this.indoor = indoor;
  }
}

const dog = new Dog("Rex", "Labrador");
const cat = new Cat("Whiskers", true);
const animal = new Animal("Generic");

console.log("dog instanceof Dog:", dog instanceof Dog);
console.log("dog instanceof Animal:", dog instanceof Animal);
console.log("dog instanceof Cat:", dog instanceof Cat);

console.log("cat instanceof Cat:", cat instanceof Cat);
console.log("cat instanceof Animal:", cat instanceof Animal);
console.log("cat instanceof Dog:", cat instanceof Dog);

console.log("animal instanceof Animal:", animal instanceof Animal);
console.log("animal instanceof Dog:", animal instanceof Dog);

// --- instanceof with deeper inheritance ---
class GuideDog extends Dog {
  handler: string;
  constructor(name: string, handler: string) {
    super(name, "Guide");
    this.handler = handler;
  }
}

const guide = new GuideDog("Buddy", "Alice");
console.log("guide instanceof GuideDog:", guide instanceof GuideDog);
console.log("guide instanceof Dog:", guide instanceof Dog);
console.log("guide instanceof Animal:", guide instanceof Animal);
console.log("guide instanceof Cat:", guide instanceof Cat);

// --- instanceof with built-in types ---
console.log("[] instanceof Array:", [] instanceof Array);
console.log("{} instanceof Object:", {} instanceof Object);
console.log("new Date instanceof Date:", new Date() instanceof Date);
console.log("new Error instanceof Error:", new Error() instanceof Error);
console.log("/re/ instanceof RegExp:", /re/ instanceof RegExp);

// --- Type narrowing ---
function describe(x: any): string {
  if (x instanceof Dog) return "Dog: " + x.breed;
  if (x instanceof Cat) return "Cat: " + (x.indoor ? "indoor" : "outdoor");
  if (x instanceof Animal) return "Animal: " + x.name;
  if (typeof x === "number") return "Number: " + x;
  if (typeof x === "string") return "String: " + x;
  return "Unknown";
}

console.log("describe dog:", describe(dog));
console.log("describe cat:", describe(cat));
console.log("describe animal:", describe(animal));
console.log("describe 42:", describe(42));
console.log("describe hello:", describe("hello"));

// --- instanceof after storing in array ---
const animals: Animal[] = [dog, cat, animal, guide];
for (let i = 0; i < animals.length; i++) {
  const a = animals[i];
  console.log(a.name, "is Dog:", a instanceof Dog, "is Cat:", a instanceof Cat);
}
