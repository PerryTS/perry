// A recursive `function*` declared inside another function must resolve its
// own name to the generator, not to a slot of the enclosing function (#13029).
function run() {
    function* walk(n: number): Generator<number> {
        if (n === 0) {
            yield 0;
            return;
        }
        for (const value of walk(n - 1)) {
            yield value + 1;
        }
    }
    console.log([...walk(2)].join(","));
}
run();

function delegating() {
    function* countdown(n: number): Generator<number> {
        if (n < 0) return;
        yield n;
        yield* countdown(n - 1);
    }
    console.log([...countdown(3)].join(","));
}
delegating();

function withSibling() {
    const base = 10;
    function* tree(depth: number): Generator<number> {
        yield base + depth;
        if (depth > 0) yield* tree(depth - 1);
    }
    function* leaf(): Generator<string> {
        yield "leaf";
    }
    console.log([...tree(2)].join(","), [...leaf()].join(","));
}
withSibling();
