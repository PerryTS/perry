// Date semantics parity: Date.UTC defaults/rebasing (#2826),
// Date setter optional arguments (#2851), Date.parse grammar (#2827).
// All inputs are FIXED (no Date.now()/new Date() without args) so output is
// deterministic across runs and machines.

function iso(d: Date): string {
  return Number.isNaN(d.getTime()) ? "Invalid" : d.toISOString();
}

// ---- #2826: Date.UTC ----
console.log("UTC() NaN:", Number.isNaN(Date.UTC()));
console.log("UTC(2020):", Date.UTC(2020));
console.log("UTC(2020,0):", Date.UTC(2020, 0));
console.log("UTC(2020,0,1):", Date.UTC(2020, 0, 1));
console.log("UTC(2020,0,0):", Date.UTC(2020, 0, 0));
console.log("UTC(0,0,1):", Date.UTC(0, 0, 1));
console.log("UTC(99,0,1):", Date.UTC(99, 0, 1));
console.log("UTC(100,0,1):", Date.UTC(100, 0, 1));
console.log("UTC(2020,12,1):", Date.UTC(2020, 12, 1));
console.log("UTC(2020,5,15,8,30,45,123):", Date.UTC(2020, 5, 15, 8, 30, 45, 123));

// ---- #2851: Date setters with optional trailing arguments ----
let d = new Date("2020-01-02T03:04:05.006Z");
console.log("setUTCFullYear ret:", d.setUTCFullYear(2021, 5, 7));
console.log("setUTCFullYear:", iso(d));

d = new Date("2020-01-02T03:04:05.006Z");
console.log("setUTCHours ret:", d.setUTCHours(8, 9, 10, 11));
console.log("setUTCHours:", iso(d));

d = new Date("2020-01-02T03:04:05.006Z");
console.log("setUTCMinutes ret:", d.setUTCMinutes(9, 10, 11));
console.log("setUTCMinutes:", iso(d));

d = new Date("2020-01-02T03:04:05.006Z");
console.log("setUTCSeconds ret:", d.setUTCSeconds(30, 500));
console.log("setUTCSeconds:", iso(d));

d = new Date("2020-01-02T03:04:05.006Z");
console.log("setUTCMonth ret:", d.setUTCMonth(11, 25));
console.log("setUTCMonth:", iso(d));

// omitted trailing args keep current fields
d = new Date("2020-01-02T03:04:05.006Z");
d.setUTCHours(8);
console.log("setUTCHours(8) only:", iso(d));

// zero-arg leading -> Invalid Date
d = new Date("2020-01-02T03:04:05.006Z");
console.log("setUTCHours() NaN:", Number.isNaN(d.setUTCHours()));
console.log("setUTCHours() date:", iso(d));

// local-time setter: compare local components (timezone-independent)
d = new Date("2020-01-02T03:04:05.006Z");
d.setFullYear(2022, 3, 9);
console.log(
  "setFullYear local y/m/d:",
  d.getFullYear(),
  d.getMonth(),
  d.getDate(),
);

// ---- #2827: Date.parse grammar (timezone-deterministic forms) ----
console.log("parse ISO Z:", Date.parse("2020-01-02T03:04:05.006Z"));
console.log("parse date-only:", Date.parse("2020-01-02"));
console.log("parse ISO offset:", Date.parse("2020-01-02T03:04:05+02:30"));
console.log("parse RFC GMT:", Date.parse("Thu, 01 Jan 1970 00:00:00 GMT"));
console.log("parse RFC no-wd:", Date.parse("01 Jan 1970 00:00:00 GMT"));
console.log("parse year-only:", Date.parse("2020"));
console.log("parse invalid NaN:", Number.isNaN(Date.parse("not a date")));
console.log("parse month-only:", Date.parse("2020-06"));
