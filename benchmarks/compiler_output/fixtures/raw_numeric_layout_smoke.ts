declare function gc(): void;

let held: any = null;

class Gauge {
  value: number = 1;
  total: number = 2;
  label: string = "stable";
}

function numericArrays(): number {
  const xs: number[] = [1, 2, 3, 4];
  xs[0] = 4;
  xs.push(5);
  let sum = xs[0] + xs[4];
  gc();

  (xs as any)[1] = "drop";
  sum += typeof (xs as any)[1] === "string" ? 17 : 0;
  xs[2] = 6;
  sum += xs[2];
  return sum;
}

function classFields(): number {
  const gauge = new Gauge();
  gauge.value = 2.5;
  gauge.total = 7.5;
  let sum = gauge.value + gauge.total;
  held = gauge;
  gc();

  (gauge as any).value = "boxed";
  sum += typeof (gauge as any).value === "string" ? 19 : 0;

  const fast = new Gauge();
  fast.value = 4.5;
  fast.total = 5.5;
  sum += fast.value + fast.total;
  held = fast;
  gc();
  return sum;
}

const arrayResult = numericArrays();
const fieldResult = classFields();
console.log("raw_numeric_layouts:" + (arrayResult + fieldResult));
