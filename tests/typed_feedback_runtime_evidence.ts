function runtimeValue(): any {
  return "not-number";
}

const numbers: number[] = [1, 2, 3];
numbers[0] = 4;
console.log(numbers[0]);

function writeArrayFallback(target: number[], value: number): void {
  target[1] = value;
  console.log((target as any)[1]);
}

writeArrayFallback(numbers, runtimeValue());

class Counter {
  value: number = 1;
}

function writeCounterSuccess(target: Counter): void {
  target.value = 2;
  console.log(target.value);
}

function writeCounterFallback(target: Counter, value: number): void {
  target.value = value;
  console.log((target as any).value);
}

const counter = new Counter();
writeCounterSuccess(counter);
writeCounterFallback(counter, runtimeValue());
