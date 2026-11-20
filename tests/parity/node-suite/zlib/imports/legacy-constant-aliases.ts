import zlib from "node:zlib";

for (
  const key of ["Z_OK", "Z_FINISH", "Z_DEFAULT_COMPRESSION", "DEFLATE", "GZIP"]
) {
  const descriptor = Object.getOwnPropertyDescriptor(zlib, key)!;
  console.log(
    key,
    (zlib as any)[key] === (zlib.constants as any)[key],
    descriptor.enumerable,
    descriptor.writable,
    descriptor.configurable,
  );
}
