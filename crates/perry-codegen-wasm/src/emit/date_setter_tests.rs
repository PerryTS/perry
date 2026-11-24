//! `new Date(y, m, ...)`, the `Date.prototype` setters and `toUTCString` on the web target.

use super::run_web_source;

// A Date built from components, then each local and UTC setter with every argument it
// takes, read back through the getters so the output does not depend on the time zone;
// `setTime` returns the new time value. An async body builds and formats a Date too.
// Expected output is Node's.
#[test]
fn date_setters_match_node() {
    assert_eq!(
        run_web_source(
            r#"
const d = new Date(Date.UTC(2020, 0, 15, 10, 20, 30, 400));
console.log(d.toUTCString());
console.log(d.setTime(86400000), d.getTime());
const f = new Date(2020, 0, 15, 10, 20, 30, 400);
console.log(f.getFullYear(), f.getMonth(), f.getDate(), f.getHours(), f.getMinutes(), f.getSeconds(), f.getMilliseconds());
console.log(new Date(2020, 5).getMonth(), new Date().getFullYear() > 2000, isNaN(new Date(undefined).getTime()));
console.log(f.setFullYear(2021) === f.getTime(), f.getFullYear());
f.setMonth(5); f.setDate(20); f.setHours(7); f.setMinutes(8); f.setSeconds(9); f.setMilliseconds(10);
console.log(f.getMonth(), f.getDate(), f.getHours(), f.getMinutes(), f.getSeconds(), f.getMilliseconds());
f.setFullYear(2019, 2, 3); f.setHours(4, 5, 6, 7);
console.log(f.getFullYear(), f.getMonth(), f.getDate(), f.getHours(), f.getMinutes(), f.getSeconds(), f.getMilliseconds());
const u = new Date(0);
u.setUTCFullYear(2001, 1, 2); u.setUTCHours(3, 4, 5, 6);
console.log(u.toISOString());
const e = new Date(2020, 0, 1);
e.setMonth(2, 3); e.setMinutes(4, 5, 6);
console.log(e.getMonth(), e.getDate(), e.getMinutes(), e.getSeconds(), e.getMilliseconds());
e.setSeconds(7, 8); u.setUTCMonth(3, 4); u.setUTCDate(5); u.setUTCMinutes(6, 7, 8);
console.log(e.getSeconds(), e.getMilliseconds(), u.toISOString());
u.setUTCSeconds(9, 10);
console.log(u.toISOString(), u.setUTCMilliseconds(11) === u.getTime(), u.getUTCMilliseconds());
async function inAsync(x: number) { return new Date(x).toUTCString() + " " + (new Date(2020, 5) instanceof Date); }
inAsync(0).then((v) => console.log(v));
"#
        ),
        "Wed, 15 Jan 2020 10:20:30 GMT\n86400000 86400000\n2020 0 15 10 20 30 400\n5 true true\ntrue 2021\n5 20 7 8 9 10\n2019 2 3 4 5 6 7\n2001-02-02T03:04:05.006Z\n2 3 4 5 6\n7 8 2001-04-05T03:06:07.008Z\n2001-04-05T03:06:09.010Z true 11\nThu, 01 Jan 1970 00:00:00 GMT true"
    );
}
