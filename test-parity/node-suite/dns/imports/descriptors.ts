import dns from "node:dns";

for (const key of ["lookup", "Resolver", "ADDRCONFIG", "NODATA", "promises"]) {
  const descriptor = Object.getOwnPropertyDescriptor(dns, key);
  if (!descriptor) {
    console.log(key + ": missing");
    continue;
  }
  console.log(
    key + ":",
    descriptor.enumerable,
    descriptor.configurable,
    "writable" in descriptor ? descriptor.writable : "accessor",
    typeof descriptor.get,
  );
}
