// Async functions (compiled to JS bridge)
async function greeting(name: string): Promise<string> {
  return "Hello, " + name + "!";
}

// Call returns promise handle
const p = greeting("World");
console.log("async called");

async function compute(x: number, y: number): Promise<number> {
  return x + y;
}

const p2 = compute(10, 20);
console.log("compute called");
console.log("done");
