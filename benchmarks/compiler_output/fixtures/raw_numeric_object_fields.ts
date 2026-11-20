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

  // Receivers no class proof covers. With only Gauge arguments, whole-program
  // facts prove the class of every receiver below and lower each access to a
  // fixed-offset slot, which deletes the checked paths this fixture gates:
  // the dynamic accessors' shape-word probe and the declared-family read.
  const parsed: any = JSON.parse('{"note":"parsed","value":0,"next":0.5}');
  forceDynamicWrite(parsed, parsed.next);
  sum += forceDynamicRead(parsed);
  const either: Gauge = sum > 1000 ? new Gauge() : (parsed as Gauge);
  sum += either.value;

  return sum;
}

console.log("raw_numeric_object_fields:" + rawNumericObjectFieldsChecksum());
