// util.parseEnv(content) — parse .env text to an object (#2514).
import util from "node:util";

const cases = [
  "A=1\nB=2",
  "A=b # c",
  "A=foo#bar",
  'A="b # c"',
  "A=",
  "A=b=c",
  "A = b ",
  "export A=b",
  "A='x y'",
  'A="l1\\nl2"',
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
