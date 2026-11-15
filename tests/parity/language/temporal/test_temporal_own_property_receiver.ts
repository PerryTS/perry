// A Temporal value is an ordinary extensible object: a script can give it own
// data properties and own accessors. A write through an own setter runs the
// setter with the Temporal value itself as `this` (not a number, not a copy),
// for every write form: static name, computed name, Reflect.set.
const T: any = (globalThis as any).Temporal;

function probe(label: string, make: () => any): void {
  const t: any = make();
  const seen: string[] = [];
  Object.defineProperty(t, "acc", {
    configurable: true,
    enumerable: false,
    get(this: any) {
      seen.push("get:" + (this === t) + ":" + typeof this);
      return 1;
    },
    set(this: any, v: any) {
      seen.push("set:" + (this === t) + ":" + typeof this + ":" + v);
    },
  });
  t.acc = 5;
  const name = "ac" + "c";
  t[name] = 6;
  Reflect.set(t, "acc", 7);
  void t.acc;
  t.plain = "p";
  t["computed" + 1] = "c";
  console.log(label, seen.join(" "));
  console.log(label, "own", JSON.stringify(Object.keys(t)), t.plain, t.computed1, "acc" in t, Object.getOwnPropertyNames(t).sort().join(","));
  delete t.plain;
  console.log(label, "after delete", JSON.stringify(Object.keys(t)), t.plain === undefined);
}

if (typeof T === "object") {
  probe("Instant", () => T.Instant.fromEpochMilliseconds(0));
  probe("PlainDate", () => T.PlainDate.from("2024-02-29"));
  probe("PlainTime", () => T.PlainTime.from("12:34:56"));
  probe("PlainDateTime", () => T.PlainDateTime.from("2024-02-29T12:34:56"));
  probe("Duration", () => T.Duration.from({ hours: 1, minutes: 30 }));
  probe("PlainYearMonth", () => T.PlainYearMonth.from("2024-02"));
}
// Positive controls: the same writes on kinds that are not Temporal.
probe("Date", () => new Date(0));
probe("Map", () => new Map());
probe("Object", () => ({}));
probe("Array", () => []);
