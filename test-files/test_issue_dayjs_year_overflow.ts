// Regression test: dayjs.format("YYYY-MM") returned "292278994-08" instead
// of "2024-01" because:
//   1. `new Date(year, month, day, ...)` (multi-arg form) silently dropped
//      everything past the first arg, so dayjs's parseDate ended up calling
//      `new Date(year_str)` which parsed "2024" as 2024 ms-since-epoch.
//   2. Inside dayjs's minified IIFE the local `var i = r[2]-1||0` shares the
//      same scope-local LocalId (10) as the outer module-level
//      `var i = "second"`. The closure-capture detector picked up the
//      outer constant, so even with the multi-arg form fixed, the call
//      received "second"/"minute" strings as the month/ms args, which
//      coerce to NaN → Invalid Date.
//
// Both bugs are exercised here: a multi-arg `new Date(...)` whose inner
// var-decl names collide with outer-scope variables.
(function () {
  var i = "outerI";
  var s = "outerS";
  function parse(date: string): Date {
    var r = date.match(
      /^(\d{4})[-/]?(\d{1,2})?[-/]?(\d{0,2})[Tt\s]*(\d{1,2})?:?(\d{1,2})?:?(\d{1,2})?[.:]?(\d+)?$/,
    );
    if (r) {
      var i = ((r[2] as any) - 1) || 0;
      var s = (r[7] || "0").substring(0, 3);
      return new Date(
        r[1] as any,
        i,
        r[3] || 1,
        r[4] || 0,
        r[5] || 0,
        r[6] || 0,
        s,
      );
    }
    return new Date(NaN);
  }

  const d = parse("2024-01-02");
  console.log("year:", d.getFullYear());
  console.log("month:", d.getMonth());
  console.log("date:", d.getDate());
})();

// Also exercise the multi-arg form with plain numeric args to confirm we
// didn't regress the common path.
{
  const d = new Date(2024, 0, 2);
  console.log("plain.year:", d.getFullYear());
  console.log("plain.month:", d.getMonth());
  console.log("plain.date:", d.getDate());
}

// And the 7-arg form with milliseconds.
{
  const d = new Date(2024, 5, 15, 12, 30, 45, 500);
  console.log("seven.year:", d.getFullYear());
  console.log("seven.month:", d.getMonth());
  console.log("seven.ms:", d.getMilliseconds());
}
