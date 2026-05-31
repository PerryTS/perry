import assert from "node:assert";

function probe(label, value) {
  try {
    const result = assert.ifError(value);
    console.log(label, "OK", result);
  } catch (err) {
    console.log(
      label,
      "THROW",
      err.name,
      err.code,
      JSON.stringify(String(err.message).split("\n")[0]),
      "actual===value",
      err.actual === value,
      "expected",
      err.expected,
      "operator",
      err.operator,
      "generated",
      err.generatedMessage,
    );
  }
}

probe("undefined", undefined);
probe("null", null);
probe("zero", 0);
probe("false", false);
probe("string", "boom");
probe("error", new Error("boom"));
