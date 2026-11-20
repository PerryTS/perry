// #5594: Annex B legacy RegExp escapes — `\<n>` decimal escapes that aren't
// backreferences become legacy octal/identity escapes (not the `\1` the regex
// engines reject), and an invalid `\c<letter>` is the literal sequence `\c`.

function check(name: string, value: boolean) {
  if (!value) {
    throw new Error(name);
  }
  console.log(name + ": ok");
}

// A `\<n>` with no matching capture group is a legacy octal escape, so the
// pattern compiles and `\2` matches \x02 (absent here) instead of throwing.
check("decimal-not-capturing", /\b(\w+) \2\b/.test("do you listen the the band") === false);

// `.source` preserves the original pattern even though it lowered to \x01.
check("leading-escape-source", /\1/.source === "\\1");
check("leading-escape-a-source", /\a/.source === "\\a");
check("trailing-escape-source", /a\1/.source === "a\\1");

// Multi-digit legacy octal in a class range: `\12`=0o12=\n, `\14`=0o14=\x0C.
const cls = /[\d][\12-\14]{1,}[^\d]/.exec("line1\n\n\n\n\nline2");
check(
  "decimal-class-range",
  cls !== null && cls[0] === "1\n\n\n\n\nl" && cls.index === 4,
);

// `\8` / `\9` are non-octal decimal escapes → literal digit.
check("non-octal-eight", /\8/.test("8") && !/\8/.test("a"));

// A real backreference still works (fancy-regex path).
const back = /(A)\1/.exec("AA");
check("real-backref", back !== null && back[0] === "AA" && back[1] === "A");

// Invalid `\c` (followed by a non-ASCII-letter) is the literal two chars `\c`.
const cyrillic = String.fromCharCode(0x0410); // Cyrillic А
const source = "\\c" + cyrillic;
const re = new RegExp(source);
check("invalid-control-no-wraparound", re.exec(String.fromCharCode(0x0410 % 32)) === null);
check("invalid-control-not-c", re.exec(source.substring(1)) === null);
check("invalid-control-matches-literal", re.exec(source) !== null);

// Inside a character class, invalid `\c` contributes literal `\` and `c`.
const classRe = new RegExp("[\\c" + cyrillic + "]");
check("invalid-control-class-backslash", classRe.exec("\\") !== null);
check("invalid-control-class-c", classRe.exec("c") !== null);

// A valid control escape still lowers to its control byte.
const ctrl = new RegExp("\\cA").exec(String.fromCharCode(1));
check("valid-control-escape", ctrl !== null && ctrl[0] === String.fromCharCode(1));
