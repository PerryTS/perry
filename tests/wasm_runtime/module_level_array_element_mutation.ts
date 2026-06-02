import { assertOutput, runWasm } from "./helpers/harness.ts";

const output = runWasm(`
const counter: number[] = [0];
const pushed: number[] = [];

function bumpPostfix(): number {
  const before = counter[0]++;
  console.log(before);
  console.log(counter[0]);
  return counter[0];
}

function bumpPrefix(): number {
  const after = ++counter[0];
  console.log(after);
  console.log(counter[0]);
  return counter[0];
}

counter[0] = counter[0] + 1;
console.log(counter[0]);
bumpPostfix();
console.log(counter[0]);
bumpPrefix();
console.log(counter[0]);
pushed.push(7);
pushed.push(8);
console.log(pushed.length);
console.log(pushed[0]);
console.log(pushed[1]);
`);

assertOutput(output, `
1
1
2
2
3
3
3
2
7
8
`);
