function reflect(receiver: any): void {
  console.log(Object.keys(receiver).join(","));
  console.log(Object.getOwnPropertyNames(receiver).join(","));
  console.log(Object.values(receiver).join(","));
  console.log(Object.entries(receiver).map(([k, v]: any) => k + ":" + v).join(","));
  let names: string[] = [];
  for (const key in receiver) names.push(key);
  console.log(names.join(","));
}
const first: any = { alpha: 1, beta: 2 };
const second: any = { alpha: 3, beta: 4, gamma: 5 };
reflect(first);
reflect(second);
const snapshot = Object.keys(first);
snapshot.reverse();
snapshot.push("other");
console.log(first.alpha, first.beta, Object.keys(first).join(","));
const numeric: any = { tail: 10, "10": 20, "2": 30, head: 40 };
Object.defineProperty(numeric, "hidden", { value: 50, enumerable: false, configurable: true });
Object.defineProperty(numeric, "head", { writable: false });
reflect(numeric);
delete numeric["2"];
numeric["2"] = 60;
reflect(numeric);
Object.defineProperty(numeric, "hidden", { enumerable: true });
reflect(numeric);
function alterDuringGet(receiver: any): void {
  delete receiver.removed;
  Object.defineProperty(receiver, "hiddenLater", { enumerable: false });
  receiver.added = 4;
}
const changing: any = {
  get start() {
    alterDuringGet(changing);
    return 1;
  },
  removed: 2,
  hiddenLater: 3,
};
console.log(Object.entries(changing).map(([k, v]: any) => k + ":" + v).join(","));
console.log(Object.keys(changing).join(","));
const array: any = [1, 2, 3];
Object.defineProperty(array, "1", { enumerable: false });
array.extra = 9;
console.log(Object.keys(array).join(","));
class Base { field = 1; }
class Derived extends Base { field = 2; other = 3; }
function hideClassField(receiver: any): void {
  Object.defineProperty(receiver, "field", { enumerable: false });
  console.log(Object.keys(receiver).join(","));
}
hideClassField(new Derived());
