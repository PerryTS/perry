const source: any[] = [1, 2, 3];
let iteratorCalls = 0;

source[Symbol.iterator] = function*(): Generator<number> {
  iteratorCalls++;
  yield 7;
  yield 8;
};

function collect(...values: number[]): string {
  return values.join(",");
}

const receiver = {
  collect(...values: number[]): string {
    return values.join(",");
  },
};

console.log("function", collect(...source), iteratorCalls);
console.log("method", receiver.collect(...source), iteratorCalls);
