let runs = 0;

async function f(): Promise<string> {
    runs++;
    await Promise.resolve();
    return "hello";
}

const one = await f();
const two = await f();
const three = await f();

if (one !== "hello" || two !== "hello" || three !== "hello") {
    throw new Error("async reentry returned " + one + "," + two + "," + three);
}

if (runs !== 3) {
    throw new Error("async body ran " + runs + " times");
}

console.log("issue-1029 ok");
