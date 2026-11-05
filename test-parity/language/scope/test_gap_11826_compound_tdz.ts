let calls = 0;
function f(): string { calls++; return "a"; }

try {
  tdz[f()] += 1;
  console.log("no throw");
} catch (e: any) {
  console.log(e.constructor.name, "f calls:", calls);
}

const tdz: any = { a: 1 };
