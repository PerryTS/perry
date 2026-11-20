import domain from "node:domain";

const d = domain.create();
const bound = d.bind(() => {});
const descriptor = Object.getOwnPropertyDescriptor(bound, "domain")!;
console.log(bound.name, bound.length);
console.log(
  bound.domain === d,
  descriptor?.enumerable,
  descriptor?.configurable,
  descriptor?.writable,
);
