// Element conversion uses ToBigInt for BigInt views and ToNumber for the
// others, so a dynamically typed BigInt round-trips through a BigInt view and
// throws a TypeError for a Number view.

function show(label: string, run: () => unknown): void {
  try {
    console.log(label, String(run()));
  } catch (e) {
    console.log(label, (e as Error).constructor.name);
  }
}

const one: any = 1n;
const two: any = 2n;

show("BigInt64Array store", () => {
  const a = new BigInt64Array(1);
  a[0] = one;
  return a[0];
});
show("BigUint64Array store", () => {
  const a = new BigUint64Array(1);
  a[0] = two;
  return a[0] + a[0];
});
show("BigInt64Array.from", () => BigInt64Array.from([one, two]).join());
show("BigInt64Array fill", () => new BigInt64Array(2).fill(one).join());
show("BigInt64Array set", () => {
  const a = new BigInt64Array(2);
  a.set([one, two]);
  return a.join();
});
show("Float64Array.from", () => Float64Array.from([one]).join());
show("Uint8Array fill", () => new Uint8Array(2).fill(one).join());
show("Int32Array set", () => new Int32Array(1).set([one]));
show("Atomics.add BigInt64Array", () => {
  const a = new BigInt64Array(new SharedArrayBuffer(16));
  Atomics.add(a, 0, one);
  Atomics.add(a, 0, two);
  return a[0];
});
show("Atomics.add Int32Array", () => Atomics.add(new Int32Array(new SharedArrayBuffer(8)), 0, one));
show("Atomics.load index", () => Atomics.load(new Int32Array(4), one));
show("DataView setBigInt64", () => {
  const d = new DataView(new ArrayBuffer(8));
  d.setBigInt64(0, one);
  return d.getBigInt64(0);
});
show("DataView setInt8", () => new DataView(new ArrayBuffer(8)).setInt8(0, one));
show("DataView offset", () => new DataView(new ArrayBuffer(8)).getInt8(one));
show("Buffer writeBigInt64LE", () => {
  const b = Buffer.alloc(8);
  b.writeBigInt64LE(one);
  return b.readBigInt64LE();
});
show("typed element mixed", () => {
  const a = new BigInt64Array([3n]);
  const v: any = a[0];
  return v * 2;
});
show("typed element compare", () => {
  const a = new BigInt64Array([3n]);
  const v: any = a[0];
  return v > 2;
});
show("typed BigInt64Array compare", () => {
  const a = new BigInt64Array([3n, 4n]);
  return a[0] > 2 && a[1] >= 4;
});
show("typed BigInt64Array loop sum", () => {
  const a = new BigInt64Array([3n, 4n, 5n]);
  let sum = 0n;
  for (let i = 0; i < a.length; i++) sum += a[i];
  return sum;
});
show("typed BigInt64Array number mix", () => {
  const a = new BigInt64Array([3n]);
  return (a[0] as any) + 1;
});
show("untyped view loop", () => {
  const views: any[] = [new BigInt64Array([1n, 2n]), new Float64Array([1, 2])];
  const out: string[] = [];
  for (const v of views) {
    try {
      let s: any = typeof v[0] === "bigint" ? 0n : 0;
      for (let i = 0; i < v.length; i++) s = s + v[i] * v[i];
      out.push(String(s));
    } catch (e) {
      out.push((e as Error).constructor.name);
    }
  }
  return out.join();
});
