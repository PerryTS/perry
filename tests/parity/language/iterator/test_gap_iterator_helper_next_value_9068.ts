// #9068: iterator-helper objects dispatch `.next()` by class id but did not
// inherit a callable `next` value.  Reading the method therefore returned
// `undefined`, which made the standard bind-and-delegate pattern impossible.

const helper: any = [1, 2].values().map((x: number) => x * 2);
console.log("type", typeof helper.next);
console.log("own", Object.prototype.hasOwnProperty.call(helper, "next"));
const direct: any = Iterator.from([7]).map((x: number) => x);
console.log("direct type", typeof direct.next);

const helperPrototype = Object.getPrototypeOf(helper);
console.log("prototype owns", Object.prototype.hasOwnProperty.call(helperPrototype, "next"));
console.log("shared prototype", Object.getPrototypeOf([3].values().map((x: number) => x)) === helperPrototype);
console.log("prototype tag", helperPrototype[Symbol.toStringTag]);

const originalNext = helper.next.bind(helper);
console.log("bound 1", JSON.stringify(originalNext()));
console.log("bound 2", JSON.stringify(originalNext()));
console.log("bound done", JSON.stringify(originalNext()));

const other: any = [5].values().map((x: number) => x + 1);
console.log("call other", JSON.stringify(helper.next.call(other)));

try {
  helper.next.call([9].values());
} catch (error: any) {
  console.log("brand", error.name);
}
