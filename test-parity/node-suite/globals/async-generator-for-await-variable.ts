async function* letters() {
  yield "a";
  await Promise.resolve();
  yield "b";
}

const collect = async (label: string, iterable: AsyncIterable<unknown>) => {
  const values: string[] = [];
  for await (const value of iterable) {
    values.push(String(value));
    if (values.length > 4) {
      values.push("guard");
      break;
    }
  }
  console.log(`${label}:`, values.join(","));
};

await collect("direct", letters());

const held = letters();
await collect("variable", held);

const runner = {
  async *run(prefix: string) {
    yield `${prefix}1`;
    await Promise.resolve();
    yield `${prefix}2`;
  },
};
await collect("method result", runner.run("m"));

const custom = {
  async *[Symbol.asyncIterator]() {
    yield "s1";
    yield Promise.resolve("s2");
  },
};
await collect("custom async iterable", custom);

const promised: unknown[] = [];
for await (const value of [Promise.resolve(1), 2]) {
  promised.push(value);
}
console.log("array promises:", promised.join(","));
