import dns from "node:dns";

for (const key of ["lookup", "Resolver", "ADDRCONFIG", "NODATA", "promises"]) {
  const descriptor = Object.getOwnPropertyDescriptor(dns, key)!;
  console.log(
    key + ":",
    descriptor.enumerable,
    descriptor.configurable,
    "writable" in descriptor ? descriptor.writable : "accessor",
    typeof descriptor.get,
  );
}
