// Gap test: representation-selection Phase 3a — canonical string locals
// (tagged-at-rest Str rep). Exercises the four correctness obligations:
//   1. alias/refcount discipline: in-place `+=` must not corrupt aliases
//   2. SSO round-trip: short ASCII strings stay correct through
//      `+=`/`.length`/`===` without per-op heap materialization
//   3. boxed→Str acceptance: a lying `string` annotation must degrade to
//      exact JS semantics, never a wrong-value coercion
//   4. non-ASCII byte-exactness through the canonical-Str fast arms
// Run: node --experimental-strip-types test_gap_repsel_canonical_str_locals.ts
// Also run with PERRY_CANONICAL_STR_LOCALS=0 and PERRY_GC_FORCE_EVACUATE=1.

function aliasDemote(): void {
  // Obligation 1: `b` aliases `a`'s heap buffer; the demote at `let b = a`
  // must force `a += "y"` to allocate fresh instead of mutating in place.
  let a = "x".repeat(3);
  const b = a;
  a += "y";
  console.log("alias:", a, b, a.length, b.length);

  // Same discipline through a scalar-replaced array element.
  let c = "z".repeat(4);
  const arr = [c];
  c += "!";
  console.log("alias-arr:", c, arr[0]);

  // And through an object field.
  let d = "q".repeat(4);
  const o = { f: d };
  d += "?";
  console.log("alias-obj:", d, o.f);
}
aliasDemote();

function accumulator(): void {
  // The += hot-loop shape (string_concat_csv kernel). Crosses the SSO→heap
  // boundary in the first iterations and grows through several in-place
  // append reallocs.
  let csv = "";
  for (let i = 0; i < 50; i++) {
    csv += String(i);
    csv += ",";
  }
  console.log("acc:", csv.length, csv.slice(0, 12), csv.slice(-6));
}
accumulator();

function ssoRoundTrip(): void {
  // Obligation 2: short JSON-key-like strings. `id`/`ab` stay ≤5 bytes.
  let k = "i";
  k += "d"; // SSO + SSO → SSO
  console.log("sso:", k, k.length, k === "id", "id" === k);
  let l = "ab";
  l += "cde"; // exactly 5 bytes — still SSO-representable
  console.log("sso5:", l, l.length, l === "abcde", l < "abcdf", l > "abcdd");
  l += "f"; // crosses to heap
  console.log("sso6:", l, l.length, l === "abcdef");
  // SSO receiver for the char-access family.
  console.log("ssochar:", k.charCodeAt(0), k.charCodeAt(1), k.at(-1), k.codePointAt(0));
  // Compare a parsed (runtime-SSO) value against a literal (heap constant).
  const parsed: string = JSON.parse('"ok"');
  console.log("ssoparse:", parsed === "ok", parsed.length, parsed < "oz");
}
ssoRoundTrip();

function lyingAnnotation(): void {
  // Obligation 3: a `string`-typed local that actually holds `undefined`
  // (annotation lie). The canonical compare arm must route to the exact
  // non-coercing equality helper — `undefined === "..."` is false — and
  // agree with both node and the pre-phase lowering. (Number-holding lies
  // are deliberately NOT asserted here: the pre-phase unified helper
  // number-coerces them, a shipped divergence from node that this phase
  // fixes only under the flag; a byte-exact gap test must hold in the
  // flag-off arm too.)
  const s: string = undefined as unknown as string;
  console.log("lie-eq:", s === "boom", s !== "boom");

  // `+=` with a lying rhs must ToString-coerce on every destination shape
  // (SSO-at-rest and heap-at-rest) — the SSO arm must not swallow the rhs.
  const lie: string = 42 as unknown as string;
  let sso: string = JSON.parse('"ab"'); // runtime-SSO destination bits
  sso += lie;
  console.log("lie-append-sso:", sso, sso.length);
  let heap = "abcdefgh"; // literal init → heap destination bits
  heap += lie;
  console.log("lie-append-heap:", heap, heap.length);
  // SSO dest + SSO-ish rhs stays on the SSO-aware concat (result may stay
  // SSO): both sides real strings.
  let sk: string = JSON.parse('"x"');
  const sv: string = JSON.parse('"y"');
  sk += sv;
  console.log("sso-sso-append:", sk, sk.length, sk === "xy");
}
lyingAnnotation();

function nonAscii(): void {
  // Obligation 4 (non-ASCII byte-exactness): multi-byte UTF-8 through the
  // canonical `+=`/`.length`/compare arms.
  let u = "é";
  u += "x";
  console.log("utf8:", u, u.length, u === "éx", u.charCodeAt(0), u.charCodeAt(1));
  let cjk = "漢";
  cjk += "字";
  console.log("cjk:", cjk, cjk.length, cjk === "漢字", cjk.codePointAt(0));
  let emoji = "";
  emoji += "\u{1F600}";
  emoji += "!";
  console.log("emoji:", emoji, emoji.length, emoji.charCodeAt(0), emoji.charCodeAt(1));
}
nonAscii();

function comparesAndScan(): void {
  // Route-compare shape: canonical local vs heap literal, both orders,
  // equality + relational.
  let method = "GET";
  method += "";
  console.log(
    "route:",
    method === "GET",
    method !== "POST",
    "GET" === method,
    method < "HEAD",
    method >= "GET"
  );
  // char-scan shape: charCodeAt over a proven-heap accumulator result.
  let payload = "";
  for (let i = 0; i < 6; i++) payload += "abz";
  let sum = 0;
  for (let i = 0; i < payload.length; i++) sum += payload.charCodeAt(i);
  console.log("scan:", payload.length, sum);
}
comparesAndScan();

function lengthShapes(): void {
  // `.length` on SSO-at-rest, heap-at-rest, and lying receivers.
  let short = "abc";
  short += "d"; // SSO
  let long = "abc";
  long += "defgh"; // heap
  console.log("len:", short.length, long.length);
  // (A number-holding `string` local's `.length` is deliberately not
  // asserted: the pre-phase lowering already returns 0 where node says
  // undefined, and the canonical slow arm reproduces the shipped behavior.)
  const und: string = undefined as unknown as string;
  console.log("len-und:", und === undefined);
}
lengthShapes();

function templateChain(): void {
  // Template-chain shape: interpolation feeding the += accumulator.
  let out = "";
  const host = "example.com";
  for (let p = 80; p < 83; p++) {
    out += `${host}:${p};`;
  }
  console.log("tmpl:", out, out.length);
}
templateChain();
