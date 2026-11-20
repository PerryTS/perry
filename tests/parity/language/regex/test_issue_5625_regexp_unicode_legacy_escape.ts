// #5625: Annex B.1.4 legacy RegExp escapes are NOT applied under the `/u`
// (and `/v`) flag — they must throw a SyntaxError at construction instead of
// being silently relaxed the way #5594 relaxes them for sloppy patterns.
// (test262 built-ins/RegExp/unicode_restricted_octal_escape +
// unicode_restricted_identity_escape_c)

function throwsSyntax(name: string, fn: () => void) {
  try {
    fn();
  } catch (e) {
    if (e instanceof SyntaxError) {
      console.log(name + ": ok");
      return;
    }
    throw new Error(name + ": expected SyntaxError, got " + e);
  }
  throw new Error(name + ": expected SyntaxError, none thrown");
}

function ok(name: string, value: boolean) {
  if (!value) {
    throw new Error(name);
  }
  console.log(name + ": ok");
}

// Decimal escapes without a matching capture group are forbidden under /u.
throwsSyntax("octal-1-u", () => { new RegExp("\\1", "u"); });
throwsSyntax("octal-7-u", () => { new RegExp("\\7", "u"); });
throwsSyntax("octal-8-u", () => { new RegExp("\\8", "u"); });
throwsSyntax("octal-9-u", () => { new RegExp("\\9", "u"); });
throwsSyntax("octal-class-1-u", () => { new RegExp("[\\1]", "u"); });
throwsSyntax("octal-class-7-u", () => { new RegExp("[\\7]", "u"); });

// Leading-zero legacy octal forms `\0DD`.
throwsSyntax("octal-00-u", () => { new RegExp("\\00", "u"); });
throwsSyntax("octal-07-u", () => { new RegExp("\\07", "u"); });
throwsSyntax("octal-class-00-u", () => { new RegExp("[\\00]", "u"); });

// `\c` not followed by an ASCII control letter is forbidden under /u.
throwsSyntax("control-bare-u", () => { new RegExp("\\c", "u"); });
throwsSyntax("control-digit-u", () => { new RegExp("\\c1", "u"); });
throwsSyntax("control-underscore-u", () => { new RegExp("\\c_", "u"); });
throwsSyntax("control-class-bare-u", () => { new RegExp("[\\c]", "u"); });
const cyrillic = String.fromCharCode(0x0410);
throwsSyntax("control-cyrillic-u", () => { new RegExp("\\c" + cyrillic, "u"); });

// The `/v` (unicodeSets) flag is just as strict.
throwsSyntax("octal-1-v", () => { new RegExp("\\1", "v"); });
throwsSyntax("control-bare-v", () => { new RegExp("\\c", "v"); });

// Constructs that stay VALID under /u must not be rejected:
//  - a real backreference to an existing group,
ok("backref-u-ok", new RegExp("(a)\\1", "u").test("aa"));
//  - bare `\0` (NUL) when not followed by a decimal digit,
ok("nul-u-ok", new RegExp("\\0", "u").test(String.fromCharCode(0)));
//  - a valid `\cA` control escape,
ok("control-A-u-ok", new RegExp("\\cA", "u").test(String.fromCharCode(1)));
//  - ordinary character-class escapes.
ok("digit-class-u-ok", new RegExp("[\\d]", "u").test("5"));

// And in sloppy (non-/u) mode the Annex B relaxation from #5594 still holds.
ok("octal-1-sloppy-ok", /\1/.source === "\\1");
// `\c` with no control letter lowers to the literal two chars `\` + `c`
// (#5594), so the pattern matches the string "\c".
ok("control-bare-sloppy-ok", new RegExp("\\c").test("\\c"));
