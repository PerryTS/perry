// #2892 hasOwnProperty own-key checks
const proto = { inherited: 1 };
const obj = Object.create(proto);
(obj as any).own = 2;

console.log(obj.hasOwnProperty("own"));
console.log(obj.hasOwnProperty("inherited"));
console.log(obj.hasOwnProperty("absent"));
console.log(Object.prototype.hasOwnProperty.call(obj, "inherited"));

// #2891 propertyIsEnumerable honors enumerable bit
const p2 = { inherited: 1 };
const o2 = Object.create(p2);
Object.defineProperty(o2, "hidden", { value: 2, enumerable: false });
Object.defineProperty(o2, "visible", { value: 3, enumerable: true });
console.log(o2.propertyIsEnumerable("visible"));
console.log(o2.propertyIsEnumerable("hidden"));
console.log(o2.propertyIsEnumerable("inherited"));
console.log(Object.prototype.propertyIsEnumerable.call(o2, "hidden"));

// #2818 ToObject in inspection statics
console.log(Object.keys("abc"));
console.log(Object.values("ab"));
console.log(Object.entries("ab"));
console.log(Object.getOwnPropertyNames("ab"));
console.log(Object.getOwnPropertyDescriptor("ab", "0"));
console.log(Object.getOwnPropertyDescriptor("ab", "length")!.value);
console.log(Object.keys(7 as any));
console.log(Object.getOwnPropertyDescriptor(7 as any, "x"));
const sd = Object.getOwnPropertyDescriptors("ab");
console.log(Object.keys(sd));
console.log(sd["1"].value, sd["1"].enumerable, sd["length"].value);
console.log(Object.getOwnPropertySymbols("ab").length);
console.log(Object.propertyIsEnumerable.call({ a: 1 }, "a"));

function throwsType(fn: () => void): string {
  try {
    fn();
    return "no throw";
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}

console.log(throwsType(() => Object.keys(null as any)));
console.log(throwsType(() => Object.values(undefined as any)));
console.log(throwsType(() => Object.entries(null as any)));
console.log(throwsType(() => Object.getOwnPropertyNames(null as any)));
console.log(throwsType(() => Object.getOwnPropertySymbols(undefined as any)));
console.log(throwsType(() => Object.getOwnPropertyDescriptor(null as any, "x")));
console.log(throwsType(() => Object.getOwnPropertyDescriptors(undefined as any)));
