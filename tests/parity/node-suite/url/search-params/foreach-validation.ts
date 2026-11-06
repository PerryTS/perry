// #3058: URLSearchParams.prototype.forEach validates its callback argument
// like Node — a missing or non-function callback throws
// TypeError [ERR_INVALID_ARG_TYPE] before any iteration happens.
const sp = new URLSearchParams("a=1&b=2");

function probe(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label, "OK");
  } catch (e: any) {
    console.log(label, "THROW", e.name, e.code, e.message);
  }
}

probe("no arg", () => sp.forEach(undefined as any));
probe("number", () => sp.forEach(123 as any));
probe("string", () => sp.forEach("x" as any));
probe("null", () => sp.forEach(null as any));

const out: string[] = [];
sp.forEach((v, k) => out.push(`${k}=${v}`));
console.log("run:", out.join("&"));
