// #9681: Array callback validation must recognize callable Proxy values
// without dereferencing their small registry ids as ClosureHeader pointers.
//
// Exercise every Array.prototype higher-order method plus sort with a plain
// function, an empty-handler Proxy of that function, a bound function, and a
// genuine non-callable. A Proxy of that non-callable pins the callability
// boundary too. Both invalid forms also pin Node's TypeError text.

const source = [3, 1, 2];

function render(value: any): string {
  const json = JSON.stringify(value);
  return json === undefined ? "undefined" : json;
}

function exercise(label: string, callback: any, invoke: any): void {
  const names = ["plain", "proxy", "bound", "non-callable", "non-callable proxy"];
  const callbacks = [
    callback,
    new Proxy(callback, {}),
    callback.bind(null),
    {},
    new Proxy({}, {}),
  ];

  for (let i = 0; i < callbacks.length; i++) {
    try {
      console.log(label + "/" + names[i] + ": " + render(invoke(callbacks[i])));
    } catch (e: any) {
      console.log(label + "/" + names[i] + ": " + e.name + ": " + e.message);
    }
  }
}

let forEachTotal = 0;
exercise(
  "forEach",
  (value: number) => {
    forEachTotal += value;
  },
  (callback: any) => {
    forEachTotal = 0;
    source.forEach(callback);
    return forEachTotal;
  },
);

exercise("map", (value: number, index: number) => value + index, (callback: any) =>
  source.map(callback),
);
exercise("filter", (value: number) => value > 1, (callback: any) =>
  source.filter(callback),
);
exercise("some", (value: number) => value === 1, (callback: any) =>
  source.some(callback),
);
exercise("every", (value: number) => value > 0, (callback: any) =>
  source.every(callback),
);
exercise("find", (value: number) => value < 3, (callback: any) =>
  source.find(callback),
);
exercise("findIndex", (value: number) => value < 3, (callback: any) =>
  source.findIndex(callback),
);
exercise("findLast", (value: number) => value < 3, (callback: any) =>
  source.findLast(callback),
);
exercise("findLastIndex", (value: number) => value < 3, (callback: any) =>
  source.findLastIndex(callback),
);
exercise("flatMap", (value: number) => [value, value * 10], (callback: any) =>
  source.flatMap(callback),
);
exercise("reduce", (acc: number, value: number) => acc + value, (callback: any) =>
  source.reduce(callback, 10),
);
exercise("reduceRight", (acc: number, value: number) => acc + value, (callback: any) =>
  source.reduceRight(callback, 10),
);
exercise("sort", (a: number, b: number) => a - b, (callback: any) =>
  [3, 1, 2].sort(callback),
);
