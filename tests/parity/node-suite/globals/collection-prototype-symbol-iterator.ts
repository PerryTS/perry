function showDescriptor(label: string, descriptor: PropertyDescriptor | undefined) {
  console.log(
    label,
    typeof descriptor?.value,
    descriptor?.value?.name,
    descriptor?.value?.length,
    descriptor?.writable,
    descriptor?.enumerable,
    descriptor?.configurable,
  );
}

console.log("map iterator typeof:", typeof Map.prototype[Symbol.iterator]);
console.log(
  "map iterator equals entries:",
  Map.prototype[Symbol.iterator] === Map.prototype.entries,
);
showDescriptor(
  "map iterator descriptor:",
  Object.getOwnPropertyDescriptor(Map.prototype, Symbol.iterator),
);
console.log(
  "map iterator next:",
  JSON.stringify(Map.prototype[Symbol.iterator].call(new Map([[1, 2]])).next()),
);
console.log(
  "map symbols include iterator:",
  Object.getOwnPropertySymbols(Map.prototype).some((sym) => sym === Symbol.iterator),
);

console.log("set iterator typeof:", typeof Set.prototype[Symbol.iterator]);
console.log(
  "set iterator equals values:",
  Set.prototype[Symbol.iterator] === Set.prototype.values,
);
showDescriptor(
  "set iterator descriptor:",
  Object.getOwnPropertyDescriptor(Set.prototype, Symbol.iterator),
);
console.log(
  "set iterator next:",
  JSON.stringify(Set.prototype[Symbol.iterator].call(new Set([3])).next()),
);
console.log(
  "set symbols include iterator:",
  Object.getOwnPropertySymbols(Set.prototype).some((sym) => sym === Symbol.iterator),
);
