// `length`, `byteLength`, `byteOffset` of offset views at one untyped site
// whose owner is detached, resized (grown and shrunk, length-tracking and
// fixed-length views, out of bounds included) or reached through a subarray
// of a subarray: the site must read the owner's live extent every time
// (IsTypedArrayOutOfBounds: an out-of-bounds or detached view reads 0).
function ext(o: any): string {
  return o.length + "/" + o.byteLength + "/" + o.byteOffset;
}
function row(tag: string, vs: any[]): void {
  console.log(tag, vs.map(ext).join(" "));
}

// Warm the site on plain fixed views.
const plain = new ArrayBuffer(64);
const warm = [new Float32Array(plain, 4, 3), new Int16Array(plain, 10), new Uint8Array(plain, 1, 7)];
for (let i = 0; i < 3; i++) row("warm", warm);

// A detached owner: every view of it reads 0/0/0.
const dab = new ArrayBuffer(48);
const dviews = [new Float64Array(dab, 8, 3), new Int32Array(dab, 4), new Uint8Array(dab, 5, 9)];
row("before detach", dviews);
dab.transfer();
row("detached", dviews);

// A resizable owner: length-tracking and fixed views, grown, shrunk past the
// fixed views' ends (out of bounds) and past a tracking view's offset.
const rab = new ArrayBuffer(32, { maxByteLength: 128 });
const rviews = [
  new Float32Array(rab, 8),
  new Float32Array(rab, 8, 4),
  new Uint16Array(rab, 2),
  new Uint16Array(rab, 30, 1),
  new Uint8Array(rab),
];
row("rab 32", rviews);
rab.resize(96);
row("grown 96", rviews);
rab.resize(20);
row("shrunk 20", rviews);
rab.resize(6);
row("shrunk 6", rviews);
rab.resize(0);
row("shrunk 0", rviews);
rab.resize(40);
row("regrown 40", rviews);

// A subarray of a subarray (offsets accumulate on the one owner), on a plain
// and on a resizable owner, then the plain owner detached.
const sab = new ArrayBuffer(80);
const s1 = new Float32Array(sab, 8, 16);
const s2 = s1.subarray(2, 12);
const s3 = s2.subarray(3, 7);
row("sub", [s1, s2, s3]);
const rab2 = new ArrayBuffer(64, { maxByteLength: 64 });
const t1 = new Int32Array(rab2, 4);
const t2 = t1.subarray(2);
const t3 = t2.subarray(1, 3);
row("sub rab", [t1, t2, t3]);
rab2.resize(24);
row("sub rab shrunk", [t1, t2, t3]);
sab.transfer();
row("sub detached", [s1, s2, s3]);
