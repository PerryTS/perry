function* inner(): Generator<number, number, unknown> {
  yield 1;
  return 3;
}

function* outer(): Generator<number, void, unknown> {
  const tail = yield* inner();
  yield tail + 1;
}

console.log("generators/yield-delegation:" + Array.from(outer()).join(","));
