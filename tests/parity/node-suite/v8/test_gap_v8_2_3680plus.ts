import v8 from "node:v8";

// #3680 — Serializer / Deserializer class round trip.
const ser = new v8.Serializer();
ser.writeHeader();
ser.writeValue({ a: 1, b: [2, 3], c: "hi" });
const buf = ser.releaseBuffer();
console.log("isBuffer:", Buffer.isBuffer(buf));

const de = new v8.Deserializer(buf);
de.readHeader();
const round = de.readValue();
console.log("roundtrip:", JSON.stringify(round));

// Primitive writers / readers.
const s2 = new v8.Serializer();
s2.writeUint32(300);
s2.writeDouble(3.5);
const b2 = s2.releaseBuffer();
const d2 = new v8.Deserializer(b2);
console.log("u32:", d2.readUint32());
console.log("double:", d2.readDouble());

// Default* subclasses behave the same.
const ds = new v8.DefaultSerializer();
ds.writeHeader();
ds.writeValue([1, "two", true, null]);
const dbuf = ds.releaseBuffer();
const dd = new v8.DefaultDeserializer(dbuf);
dd.readHeader();
console.log("default roundtrip:", JSON.stringify(dd.readValue()));

console.log("Serializer typeof:", typeof v8.Serializer);
console.log("Deserializer typeof:", typeof v8.Deserializer);

// #3679 — lifecycle + diagnostic-control surface shapes.
console.log("promiseHooks typeof:", typeof v8.promiseHooks);
console.log("startupSnapshot typeof:", typeof v8.startupSnapshot);
console.log("isBuildingSnapshot typeof:", typeof v8.startupSnapshot.isBuildingSnapshot);
console.log("isBuildingSnapshot():", v8.startupSnapshot.isBuildingSnapshot());
console.log("setFlagsFromString typeof:", typeof v8.setFlagsFromString);
console.log("setFlagsFromString():", v8.setFlagsFromString("--max_old_space_size=100"));
console.log("takeCoverage typeof:", typeof v8.takeCoverage);
console.log("takeCoverage():", v8.takeCoverage());
console.log("stopCoverage typeof:", typeof v8.stopCoverage);
console.log("stopCoverage():", v8.stopCoverage());
console.log("onInit typeof:", typeof v8.promiseHooks.onInit);
const stop = v8.promiseHooks.onInit(() => {});
console.log("onInit returns typeof:", typeof stop);
console.log("createHook typeof:", typeof v8.promiseHooks.createHook);
