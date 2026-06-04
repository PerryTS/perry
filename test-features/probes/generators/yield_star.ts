function* inner() {
    yield 1;
    yield 2;
    return 3;
}

function* outer() {
    const result = yield* inner();
    yield result;
}

console.log(`generator:${Array.from(outer()).join(",")}`);
