// Regression: date-fns format() needs `string.match()` to honor
// fancy-regex fallback for patterns with backreferences (`(\w)\1*`).
// Before this fix, `formatStr.match(formattingTokensRegExp)` returned
// null because the regex crate rejected the pattern and the fancy
// fallback was only wired into `RegExp.prototype.exec()`. The downstream
// `.map(...)` then crashed with NULL_PTR_METHOD_CALL.
//
// We don't import date-fns here (it's a compilePackages dep, not a test
// dep). Instead, exercise the underlying regex shape that drives format().

const formattingTokensRegExp =
  /[yYQqMLwIdDecihHKkms]o|(\w)\1*|''|'(''|[^'])+('|$)|./g;

const tokens = "yyyy-MM-dd HH:mm:ss".match(formattingTokensRegExp);
console.log(tokens);

const tokens2 = "EEEE, MMMM do yyyy".match(formattingTokensRegExp);
console.log(tokens2);

// Backreference + non-global (returns first match w/ captures)
const simple = /(\w)\1*/;
console.log("abcaaab".match(simple));

// Backreference + global
const g = /(\w)\1*/g;
console.log("aabbbccdd".match(g));

// No-match returns null
console.log("xyz".match(/(\w)\1+/));

// End-to-end: format(date, pattern). Uses the native runtime path
// (date-fns is well-known-aliased to perry-ext-dayjs).
import { format } from "date-fns";
const d = new Date(2020, 0, 6);
console.log(format(d, "yyyy-MM-dd"));
console.log(format(d, "yyyy"));
console.log(format(d, "MM"));
console.log(format(d, "dd"));
console.log(typeof format(d, "yyyy"));
// Composed patterns + month/weekday names + ordinal suffix.
// All match `node --experimental-strip-types` byte-for-byte.
console.log(format(d, "yyyy-MM-dd HH:mm:ss"));
console.log(format(d, "MMMM do yyyy"));
console.log(format(d, "EEEE"));
// Ordinal-only tokens.
console.log(format(d, "do"));
console.log(format(d, "Mo"));
console.log(format(d, "yo"));
// AM/PM variants (run length controls casing/dotting).
const pmDate = new Date(2020, 0, 6, 13, 45, 30);
console.log(format(pmDate, "a"));
console.log(format(pmDate, "aaa"));
console.log(format(pmDate, "aaaa"));
console.log(format(pmDate, "aaaaa"));
