function hasOwn(obj: any, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(obj, key);
}

function summarizeArray(value: any): string {
  const parts: string[] = [];
  for (let i = 0; i < value.length; i++) {
    parts.push(hasOwn(value, String(i)) ? String(value[i]) : "<hole>");
  }
  return `len=${value.length};keys=${Object.keys(value).join(",")};values=${parts.join("|")}`;
}

function show(label: string, value: any): void {
  if (Array.isArray(value)) {
    console.log(`${label}:${summarizeArray(value)}`);
  } else {
    console.log(`${label}:${String(value)}`);
  }
}

function showThrow(label: string, fn: () => any): void {
  try {
    show(label, fn());
  } catch (error: any) {
    console.log(`${label}:throw:${error.name}`);
  }
}

const sparseLike = { length: 3, 0: "a", 2: "c" };

show("join.sparseLike", Array.prototype.join.call(sparseLike, "|"));
show(
  "map.sparseLike",
  Array.prototype.map.call(sparseLike, (value: any, index: number) => `${index}:${String(value)}`),
);
show(
  "filter.sparseLike",
  Array.prototype.filter.call(sparseLike, (_value: any, index: number) => index >= 1),
);
show(
  "slice.sparseLike",
  Array.prototype.slice.call(sparseLike, 0, 3),
);
show(
  "indexOf.missingUndefined",
  Array.prototype.indexOf.call({ length: 1 }, undefined),
);
show(
  "includes.missingUndefined",
  Array.prototype.includes.call({ length: 1 }, undefined),
);
show(
  "reduce.sparseLike",
  Array.prototype.reduce.call(
    sparseLike,
    (acc: string, value: any, index: number) => acc + `:${index}:${String(value)}`,
    "seed",
  ),
);

const liveLike: any = { length: 3, 0: "a", 2: "c" };
const visits: string[] = [];
Array.prototype.forEach.call(liveLike, (value: any, index: number) => {
  visits.push(`${index}:${String(value)}`);
  if (index === 0) {
    liveLike[1] = "b";
  }
});
show("forEach.liveMutation", visits.join("|"));

const thisArgResult = Array.prototype.map.call(
  { length: 1, 0: 2 },
  function (this: any, value: number) {
    return this.scale * value;
  },
  { scale: 3 },
);
show("map.thisArg", thisArgResult);

show(
  "some.thisArg",
  Array.prototype.some.call(
    { length: 1, 0: 4 },
    function (this: any, value: number) {
      return value === this.target;
    },
    { target: 4 },
  ),
);

show(
  "find.holeVisited",
  Array.prototype.find.call({ length: 2, 1: "x" }, (value: any) => value === undefined),
);
show("join.string", Array.prototype.join.call("abc", "-"));
show(
  "map.string",
  Array.prototype.map.call("ab", (value: any, index: number) => `${value}${index}`),
);

const borrowedJoin = Array.prototype.join;
show("join.borrowedCall", borrowedJoin.call(sparseLike, "/"));

const borrowedSlice = Array.prototype.slice;
show("slice.borrowedApply", borrowedSlice.apply(sparseLike, [1, 3]));

showThrow("join.null", () => Array.prototype.join.call(null));
showThrow("map.badCallback", () => Array.prototype.map.call({ length: 1, 0: 1 }, 5 as any));
