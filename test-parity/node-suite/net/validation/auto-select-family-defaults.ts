import * as net from "node:net";

function label(value: any): string {
  if (typeof value === "number" && Number.isNaN(value)) {
    return "NaN";
  }
  return String(value);
}

function firstLine(err: any): string {
  return String(err.message).split("\n")[0];
}

const familyValues: any[] = [true, false, 1, 0, "true", null, undefined];
for (const value of familyValues) {
  try {
    const before = net.getDefaultAutoSelectFamily();
    const result = net.setDefaultAutoSelectFamily(value);
    console.log("family", label(value), "OK", net.getDefaultAutoSelectFamily(), result);
    net.setDefaultAutoSelectFamily(before);
  } catch (err: any) {
    console.log("family", label(value), "THROW", err.name, err.code, "|", firstLine(err));
  }
}

const timeoutValues: any[] = [
  1,
  9,
  10,
  0,
  -1,
  NaN,
  Infinity,
  "5",
  null,
  undefined,
  1.5,
  2147483647,
  2147483648,
];
for (const value of timeoutValues) {
  try {
    const before = net.getDefaultAutoSelectFamilyAttemptTimeout();
    const result = net.setDefaultAutoSelectFamilyAttemptTimeout(value);
    console.log(
      "timeout",
      label(value),
      "OK",
      net.getDefaultAutoSelectFamilyAttemptTimeout(),
      result,
    );
    net.setDefaultAutoSelectFamilyAttemptTimeout(before);
  } catch (err: any) {
    console.log("timeout", label(value), "THROW", err.name, err.code, "|", firstLine(err));
  }
}
