function logRepeat(label: string, count: any) {
  try {
    console.log(label, "ok", JSON.stringify("ab".repeat(count)));
  } catch (err: any) {
    console.log(label, "throw", err.name, err.message, err instanceof RangeError);
  }
}

logRepeat("three", 3);
logRepeat("zero", 0);
logRepeat("negative", -1);
logRepeat("infinity", Infinity);
logRepeat("nan", NaN);
logRepeat("fraction", 1.9);
logRepeat("string-number", "2");
logRepeat("string-nonnumeric", "x");
logRepeat("null", null);
logRepeat("undefined", undefined);
