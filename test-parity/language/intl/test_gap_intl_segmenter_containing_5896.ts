// #5896: Intl.Segmenter returns a branded Segments collection whose
// containing() method resolves UTF-16 indices and preserves lone surrogates.

const wordSegments: any = new Intl.Segmenter("en", { granularity: "word" }).segment(
  "hello world",
);

for (const index of [0, 5, 6, 10, -1, 11]) {
  const record = wordSegments.containing(index);
  console.log(
    index,
    record === undefined
      ? "undefined"
      : `${record.index}|${record.segment}|${record.input}|${record.isWordLike}`,
  );
}

const coerced = wordSegments.containing({ valueOf: () => 6 });
console.log("coerced", coerced.index, coerced.segment);

const containing: any = wordSegments.containing;
console.log("builtin", containing.name, containing.length);
for (const receiver of [undefined, null, {}, []]) {
  try {
    containing.call(receiver, 0);
    console.log("branding", "no throw");
  } catch (error) {
    console.log("branding", (error as Error).constructor.name);
  }
}

const loneInput = "\ud800 ";
const loneRecord: any = new Intl.Segmenter("en").segment(loneInput).containing(0);
console.log(
  "lone",
  loneRecord.segment.length,
  loneRecord.segment.charCodeAt(0),
  loneRecord.input === loneInput,
);
