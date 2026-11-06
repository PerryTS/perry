// #10639: a URL subclass must receive URL's internal state while preserving
// its declared prototype and fields. Both the implicit derived constructor and
// a written `super(input, base)` call used to leave hostname/pathname undefined.

class ImplicitUrl extends URL {
  kind = "implicit";
}

const implicit = new ImplicitUrl("child?q=1", "https://example.com/base/");
console.log(
  "implicit:",
  implicit.hostname,
  implicit.pathname,
  implicit.search,
  implicit.kind,
  implicit instanceof ImplicitUrl,
  implicit instanceof URL,
);

class ExplicitUrl extends URL {
  kind: string;

  constructor(input: string, base: string) {
    super(input, base);
    this.kind = "explicit";
  }
}

const explicit = new ExplicitUrl("../next#hash", "https://perry.dev/a/b/");
console.log(
  "explicit:",
  explicit.hostname,
  explicit.pathname,
  explicit.hash,
  explicit.kind,
  explicit instanceof ExplicitUrl,
  explicit instanceof URL,
);
