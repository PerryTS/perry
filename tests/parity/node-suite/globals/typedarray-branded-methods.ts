// @ts-nocheck

function showError(label, fn) {
  try {
    fn();
    console.log(label + ":NO_THROW");
  } catch (error) {
    console.log(label + ":" + error.constructor.name);
  }
}

function showView(label, value) {
  console.log(
    label + ":" + value.constructor.name + ":" + Array.from(value).join(","),
  );
}

function showMutating(label, method, receiver, ...args) {
  const returned = Int16Array.prototype[method].call(receiver, ...args);
  console.log(
    label +
      ":" +
      (returned === receiver) +
      ":" +
      receiver.constructor.name +
      ":" +
      Array.from(receiver).join(","),
  );
}

const nonTypedArrayCases = [
  ["map", [function (value) { return value; }]],
  ["filter", [function () { return true; }]],
  ["slice", [0, 1]],
  ["subarray", [0, 1]],
  ["copyWithin", [0, 1]],
  ["fill", [7]],
  ["reverse", []],
  ["sort", []],
  ["toReversed", []],
  ["toSorted", []],
  ["findIndex", [function () { return true; }]],
  ["findLastIndex", [function () { return true; }]],
  ["set", [[1], 0]],
];

for (const [method, args] of nonTypedArrayCases) {
  showError(method + " array receiver", function () {
    Int16Array.prototype[method].call([1, 2, 3], ...args);
  });
  showError(method + " inherited receiver", function () {
    Int16Array.prototype[method].call(Object.create(Uint8Array.prototype), ...args);
  });
}

showView(
  "map borrowed",
  Int16Array.prototype.map.call(new Uint8Array([3, 4]), function (value) {
    return value + 1;
  }),
);
showView(
  "filter borrowed",
  Int16Array.prototype.filter.call(new Uint8Array([1, 2, 3]), function (value) {
    return value > 1;
  }),
);
showView("slice borrowed", Int16Array.prototype.slice.call(new Uint8Array([5, 6, 7]), 1));
showView(
  "subarray borrowed",
  Int16Array.prototype.subarray.call(new Uint8Array([8, 9, 10]), 1),
);
showView(
  "toReversed borrowed",
  Int16Array.prototype.toReversed.call(new Uint8Array([1, 2, 3])),
);
showView(
  "toSorted borrowed",
  Int16Array.prototype.toSorted.call(new Uint8Array([3, 1, 2])),
);

showMutating("copyWithin borrowed", "copyWithin", new Uint8Array([1, 2, 3]), 0, 1);
showMutating("fill borrowed", "fill", new Uint8Array([1, 2, 3]), 9, 1);
showMutating("reverse borrowed", "reverse", new Uint8Array([1, 2, 3]));
showMutating("sort borrowed", "sort", new Uint8Array([3, 1, 2]));

showView(
  "buffer map borrowed",
  Int16Array.prototype.map.call(Buffer.from([3, 4]), function (value) {
    return value + 1;
  }),
);
showView("buffer slice borrowed", Int16Array.prototype.slice.call(Buffer.from([5, 6, 7]), 1));

const bufferFillReceiver = Buffer.from([1, 2, 3]);
const bufferFillReturned = Int16Array.prototype.fill.call(bufferFillReceiver, 9, 1);
console.log(
  "buffer fill borrowed:" +
    (bufferFillReturned === bufferFillReceiver) +
    ":" +
    bufferFillReceiver.constructor.name +
    ":" +
    Array.from(bufferFillReceiver).join(","),
);

const setReceiver = new Uint8Array([0, 0, 0]);
const setReturned = Int16Array.prototype.set.call(setReceiver, [4, 5], 1);
console.log(
  "set borrowed:" +
    String(setReturned) +
    ":" +
    setReceiver.constructor.name +
    ":" +
    Array.from(setReceiver).join(","),
);

console.log(
  "findIndex borrowed:" +
    Int16Array.prototype.findIndex.call(new Uint8Array([1, 2, 3]), function (value) {
      return value > 1;
    }),
);
console.log(
  "findLastIndex borrowed:" +
    Int16Array.prototype.findLastIndex.call(new Uint8Array([1, 2, 3]), function (value) {
      return value < 3;
    }),
);
