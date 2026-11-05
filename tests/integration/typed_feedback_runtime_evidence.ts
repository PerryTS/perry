function runtimeValue(): any {
  return "not-number";
}

const numbers: number[] = [1, 2, 3];
numbers[0] = 4;
console.log(numbers[0]);

function pushNumber(target: number[]): void {
  target.push(5);
  console.log(target[3]);
}

pushNumber(numbers);

function writeArrayFallback(target: number[], value: number): void {
  target[1] = value;
  console.log(target[1]);
}

writeArrayFallback(numbers, runtimeValue());

function writeObjectFast(target: any): void {
  target.fast = 6;
  console.log(target.fast);
}

writeObjectFast({});

function packedLoopChecksum(): number {
  const values: number[] = [1.5, 2.25, 3.75, 4.5, 6.0];
  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    sum = sum + values[i];
  }
  for (let i = 0; i < values.length; i++) {
    values[i] = values[i] * 2 + i;
  }
  return sum + values[0];
}

console.log(packedLoopChecksum());

class DirectShapeMethod {
  value: number = 31;
  read(): number {
    return this.value;
  }
}

console.log(new DirectShapeMethod().read());

class Counter {
  value: number = 1;
}

function writeCounterSuccess(target: Counter): void {
  target.value = 2;
  console.log(target.value);
}

function writeCounterFallback(target: Counter, value: number): void {
  target.value = value;
  console.log(target.value);
}

const counter = new Counter();
writeCounterSuccess(counter);
writeCounterFallback(counter, runtimeValue());
