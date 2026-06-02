// util.parseEnv(content) — parse .env text to an object (#2514).
import util from "node:util";

const cases = [
  "A=1\nB=2",
  "A=b # c",
  "A=abc#def",
  "A=foo#bar",
  'A="b # c"',
  "A=",
  "A=b=c",
  "A = b ",
  "export A=b",
  "A='x y'",
  'A="l1\\nl2"',
  'A="l1\nl2"',
  'A="a\\nb"\nB="a\\tb"\nC="a\\\\b"',
  "JUSTKEY\nA=1",
  "\n# hi\n  # ind\nA=1",
  "A=1\nA=2",
  'A="one\ntwo"\nB=3',
  "A='one\ntwo'\nB=3",
  "A=`one\ntwo`\nB=3",
  'A="one\r\ntwo"\r\nB=3',
  'A="one\nB=2',
  'DB="postgres://u:p@h/db"\nPORT=5432 # default\nNAME=app',
];
for (const c of cases) {
  const r = util.parseEnv(c);
  console.log(JSON.stringify(r), "|", Object.keys(r).join(","));
}

const mixed = [
  "A=1",
  "B = two # comment",
  'C="three # not comment"',
  "D=unquoted value # comment",
  "export E=5",
  'MULTI="line1',
  'line2"',
  "SINGLE='hash # stays'",
  "BACK=`tick # stays`",
  "BAD-NAME=bad",
  "A=last",
].join("\n");
const parsed = util.parseEnv(mixed);
for (const key of Object.keys(parsed).sort()) {
  console.log("mixed", key + ":", JSON.stringify(parsed[key]));
}

function reportInvalid(label, value) {
  try {
    util.parseEnv(value);
    console.log("invalid", label, "OK");
  } catch (err) {
    const e = err;
    console.log(
      "invalid",
      label,
      e.name,
      e.code || "nocode",
      String(e.message).split("\n")[0],
    );
  }
}

reportInvalid("undefined", undefined);
reportInvalid("null", null);
reportInvalid("number", 123);
reportInvalid("boolean", true);
reportInvalid("object", {});
reportInvalid("array", []);
