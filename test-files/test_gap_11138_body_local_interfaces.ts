// Body-local interfaces must keep user methods whose names overlap Array's.
function makeStack(): any {
  return {
    count: 0,
    push(value: number) { this.count += value; return this.count; },
    size() { return this.count; },
  };
}

function local(make: any): void {
  interface Stack { push(value: number): number; size(): number }
  const stack: Stack = make();
  console.log("local", stack.push(2), stack.size());
  {
    interface Stack<T> { push(value: T): number; size(): number }
    const inner: Stack<number> = make();
    console.log("generic block", inner.push(3), inner.size());
  }
  console.log("outer again", stack.push(4), stack.size());
}

function forward(make: any): void {
  function nested(): void {
    const stack: Stack = make();
    console.log("nested", stack.push(5), stack.size());
  }
  const arrow = () => {
    const stack: Stack = make();
    console.log("arrow", stack.push(6), stack.size());
  };
  const stack: Stack = make();
  console.log("forward", stack.push(7), stack.size());
  interface Stack { push(value: number): number; size(): number }
  nested();
  arrow();
}

function switched(make: any): void {
  switch (1) {
    case 1:
      const stack: Stack = make();
      console.log("switch", stack.push(8), stack.size());
      break;
    default:
      // All cases share one lexical type scope, even before the declaration.
      interface Stack { push(value: number): number; size(): number }
  }
}

local(makeStack);
forward(makeStack);
switched(makeStack);
const numbers: number[] = [];
console.log("array", numbers.push(9), numbers[0]);
