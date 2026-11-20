let first: any;

function show(label: string, value: unknown) {
  console.log(label + ": " + String(value));
}

function tag(strings: TemplateStringsArray, value: unknown) {
  show("same", first === undefined ? "first" : first === strings);
  if (first === undefined) {
    first = strings;
  }
  show("raw same", strings.raw === first.raw);
  show("frozen", Object.isFrozen(strings) + "|" + Object.isFrozen(strings.raw));
  show("extensible", Object.isExtensible(strings) + "|" + Object.isExtensible(strings.raw));
  show("raw enumerable", Object.getOwnPropertyDescriptor(strings, "raw")?.enumerable);

  try {
    (strings as any)[0] = "x";
  } catch {}
  try {
    (strings.raw as any)[0] = "x";
  } catch {}
  try {
    (strings as any).length = 9;
  } catch {}
  try {
    (strings as any).extra = "x";
  } catch {}

  show(
    "values",
    JSON.stringify([
      strings[0],
      strings[1],
      strings.raw[0],
      strings.raw[1],
      strings.length,
      (strings as any).extra,
    ]),
  );
  show("value", value);
}

function call(value: unknown) {
  tag`a\n${value}b`;
}

call(1);
call(2);

let duplicateSiteFirst: any;

function duplicateSite(strings: TemplateStringsArray) {
  show(
    "duplicate site same",
    duplicateSiteFirst === undefined ? "first" : duplicateSiteFirst === strings,
  );
  if (duplicateSiteFirst === undefined) {
    duplicateSiteFirst = strings;
  }
}

duplicateSite`same`;
duplicateSite`same`;
