function show(label: string, value: unknown) {
  console.log(label + ":", String(value));
}

function isFiniteNumber(value: unknown): boolean {
  return typeof value === "number" && Number.isFinite(value);
}

const detachedNow = Date.now;
const detachedParse = Date.parse;
const detachedUTC = Date.UTC;

show("typeof Date.now", typeof Date.now);
show("Date.now name", Date.now.name);
show("Date.now length", Date.now.length);
show("typeof Date.parse", typeof Date.parse);
show("Date.parse name", Date.parse.name);
show("Date.parse length", Date.parse.length);
show("typeof Date.UTC", typeof Date.UTC);
show("Date.UTC name", Date.UTC.name);
show("Date.UTC length", Date.UTC.length);

show("direct now finite", isFiniteNumber(Date.now()));
show("detached now type", typeof detachedNow);
show("detached now finite", isFiniteNumber(detachedNow()));
show("now.call object finite", isFiniteNumber(Date.now.call({})));
show("now.bind object finite", isFiniteNumber(Date.now.bind({})()));

show("direct parse epoch", Date.parse("1970-01-01T00:00:00.000Z"));
show("detached parse day2", detachedParse("1970-01-02T00:00:00.000Z"));
show("parse.call object day3", Date.parse.call({}, "1970-01-03T00:00:00.000Z"));

show("direct UTC", Date.UTC(2020, 0, 2, 3, 4, 5, 6));
show("detached UTC", detachedUTC(2020, 0, 2, 3, 4, 5, 6));
show("UTC.call object", Date.UTC.call({}, 1970, 0, 1, 0, 0, 0, 1));
show("UTC.apply object", Date.UTC.apply({}, [1970, 0, 1, 0, 0, 0, 2]));
show("UTC.bind object", Date.UTC.bind({})(1970, 0, 1, 0, 0, 0, 3));
