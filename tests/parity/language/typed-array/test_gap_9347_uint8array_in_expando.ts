// Uint8Array uses Perry's buffer representation while Int32Array uses the
// typed-array representation.  Ordinary own properties must be visible to the
// `in` operator on both, including when the write is type-erased.
function inspect(label: string, value: Uint8Array | Int32Array): void {
  (value as any).extra = 9;
  console.log(
    label,
    "extra" in value,
    value.hasOwnProperty("extra"),
    Object.keys(value).join(","),
  );
}

inspect("i32", new Int32Array(1));
inspect("u8", new Uint8Array(1));
