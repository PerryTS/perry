function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", String(fn()));
  } catch (e: any) {
    console.log(label + ":", e?.name + ":" + e?.message);
  }
}

const highNonIndex = 4294967295;
const maxIndex = 4294967294;
const overIndex = 4294967296;

const a = [0, 1, 2];
a[highNonIndex] = "x";
show("2^32-1 length", () => a.length);
show("2^32-1 value", () => a[highNonIndex]);
show("2^32-1 own", () => Object.prototype.hasOwnProperty.call(a, String(highNonIndex)));

const b = [0, 1, 2];
b[maxIndex] = "y";
show("2^32-2 length", () => b.length);
show("2^32-2 value", () => b[maxIndex]);
show("2^32-2 own", () => Object.prototype.hasOwnProperty.call(b, String(maxIndex)));
show("2^32-2 keys", () => Object.keys(b).join("|"));

const c = [0, 1, 2];
c[overIndex] = "z";
show("2^32 length", () => c.length);
show("2^32 value", () => c[overIndex]);
show("2^32 own", () => Object.prototype.hasOwnProperty.call(c, String(overIndex)));

const d = [0, 1, 2];
d[String(highNonIndex)] = "s";
show("string 2^32-1 length", () => d.length);
show("string 2^32-1 value", () => d[String(highNonIndex)]);
