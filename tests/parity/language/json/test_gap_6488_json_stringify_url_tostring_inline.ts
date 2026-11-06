// #6488: JSON.stringify(u.toString()) returned undefined when the
// URL.prototype.toString()/toJSON() call was inlined directly as the
// argument. The single-arg JSON.stringify HIR lowering pattern-matched
// the UrlInstanceToString/UrlInstanceToJSON variants as "the argument
// is a URL object" and wrapped the href STRING in another
// UrlInstanceToJSON, so js_url_get_href read a StringHeader as an
// ObjectHeader and produced undefined. Only Expr::UrlNew proves the
// lowered argument is a URL object.
const u = new URL("http://x/en", "http://x/");

// The bug: inline string-producing URL method calls.
console.log(JSON.stringify(u.toString()));
console.log(JSON.stringify(u.toJSON()));

// Stored form (always worked) — must stay identical to the inline form.
const r = u.toString();
console.log(JSON.stringify(r));

// URL-object arguments — the toJSON interception must still fire so
// these stringify as the quoted href, not a field dump.
console.log(JSON.stringify(u));
console.log(JSON.stringify(new URL("http://y/fr", "http://y/")));

// Replacer/spacer forms: toJSON runs BEFORE the replacer per
// SerializeJSONProperty, so these also print the quoted href (perry
// used to walk the opaque URL object and throw a circular-structure
// TypeError on its searchParams back-reference).
console.log(JSON.stringify(u, null, 2));
console.log(JSON.stringify(u, (_k: string, v: unknown) => v, 2));

// Property read producing the href string.
console.log(JSON.stringify(u.href));
