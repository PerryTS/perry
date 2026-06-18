class Gauge {
  value: number = 1.5;
  total: number = 2.5;
  note: any = "stable";
}

function forceDynamicRead(gauge: any): number {
  return gauge.value;
}

function forceDynamicWrite(gauge: any, value: any): void {
  gauge.value = value;
}

function rawNumericObjectFieldsChecksum(): number {
  const fast = new Gauge();
  fast.value = 4.5;
  fast.total = 7.5;
  let sum = fast.value + fast.total;

  const fallback = new Gauge();
  forceDynamicWrite(fallback, "boxed");
  sum += typeof (fallback as any).value === "string" ? 11 : 0;
  forceDynamicWrite(fallback, 3.25);
  sum += forceDynamicRead(fallback);

  return sum;
}

console.log("raw_numeric_object_fields:" + rawNumericObjectFieldsChecksum());
