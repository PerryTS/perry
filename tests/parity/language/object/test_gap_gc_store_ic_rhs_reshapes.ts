// The static-key store IC reads the receiver's ShapeId AFTER the RHS. An RHS
// that changes the receiver's shape — deletes or adds a key, installs a
// setter, makes the key non-writable, freezes — must send the store to the
// full [[Set]], even though the site was primed on the receiver's shape as it
// was BEFORE the RHS ran. Each site below is primed on plain receivers first,
// then fed a receiver of the same shape whose RHS reshapes it.

const log: string[] = [];

// ONE store site. Its RHS dispatches on `mode`: mode 0 is inert (priming),
// every other mode reshapes the receiver before returning the value.
function store(o: any, mode: number, v: number): void {
  o.k = act(o, mode, v);
}

function act(o: any, mode: number, v: number): number {
  switch (mode) {
    case 1:
      delete o.a;
      break;
    case 2:
      delete o.k;
      break;
    case 3:
      o.z = 1;
      break;
    case 4:
      Object.defineProperty(o, "k", {
        set(x: number) {
          log.push("setter saw " + x);
        },
        get() {
          return "from-getter";
        },
        configurable: true,
      });
      break;
    case 5:
      Object.defineProperty(o, "k", { writable: false });
      break;
    case 6:
      Object.freeze(o);
      break;
    case 7:
      Object.seal(o);
      break;
    case 8:
      Object.preventExtensions(o);
      break;
    case 9:
      delete o.k;
      Object.setPrototypeOf(o, {
        set k(x: number) {
          log.push("proto setter saw " + x);
        },
      });
      break;
  }
  return v;
}

const labels = [
  "",
  "delete-other",
  "delete-self",
  "add-key",
  "setter",
  "non-writable",
  "freeze",
  "seal",
  "prevent-extensions",
  "proto-setter-after-delete",
];

// ONE call site feeds every receiver, so the site the reshaping receiver
// reaches is exactly the site its plain siblings primed (a call site per mode
// could be cloned and specialised, and would never be primed).
const work: { o: any; mode: number; v: number }[] = [];
for (let mode = 1; mode < labels.length; mode++) {
  for (let i = 0; i < 8; i++) work.push({ o: { a: 1, k: 0, b: 2 }, mode: 0, v: i });
  work.push({ o: { a: 1, k: 0, b: 2 }, mode, v: 10 + mode });
}
for (const w of work) {
  let outcome = "stored";
  try {
    store(w.o, w.mode, w.v);
  } catch (e) {
    outcome = "threw " + (e as Error).constructor.name;
  }
  const o = w.o;
  if (w.mode === 0) {
    if (o.k !== w.v) log.push("prime lost " + w.v);
    continue;
  }
  const desc = Object.getOwnPropertyDescriptor(o, "k");
  log.push(
    labels[w.mode] + ": " + outcome + " k=" + String(o.k) + " keys=" + Object.keys(o).join("|") +
      " writable=" + String(desc && desc.writable) + " frozen=" + Object.isFrozen(o),
  );
}
for (const line of log) console.log(line);
