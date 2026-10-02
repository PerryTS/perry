// jws/jsonwebtoken shape: split a JWT on "." (one-character STRING separator)
const variant = process.argv[2] || "split"; const N = Number(process.argv[3] || "300000");
const tok = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ1MSIsInJvbGUiOiJhZG1pbiIsImlhdCI6MTcwMDAwMDAwMCwiZXhwIjoxNzAwMDAzNjAwLCJhdWQiOiJhcGkiLCJpc3MiOiJhdXRoLmV4YW1wbGUifQ.ndKK8_0z8Qw_OyczXXftx3rfjSEDizNo8Sb5awzd7Fw";
const anyTok: any = tok, dot: any = ".";
const ops: Record<string, () => number> = {
  split: () => tok.split(".")[1].length,                    // 179 chars, 3 pieces
  split_any: () => anyTok.split(dot)[1].length,             // untyped receiver and separator
  split_limit1: () => tok.split(".", 1)[0].length,          // jws decode: stop after the first piece
  split_short: () => "a.b.c".split(".").length,             // 5 chars, 3 pieces
  indexof: () => { const a = tok.indexOf("."), b = tok.indexOf(".", a + 1); return tok.slice(a + 1, b).length; }, // control
  indexof_limit1: () => tok.slice(0, tok.indexOf(".")).length, // control for split_limit1
};
const op = ops[variant];
function run(n: number): number { let acc = 0; for (let i = 0; i < n; i++) acc += op(); return acc; }
run(N / 5 | 0); const t0 = performance.now(); const cs = run(N);
console.log(`variant=${variant} checksum=${cs} ms=${(performance.now() - t0).toFixed(2)}`);