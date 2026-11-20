// Static field initializers and static blocks must run for a class
// declared INSIDE a function body, not only for top-level classes.
// Regression test: previously the in-function lowering path emitted the
// class but not its static-field-init / static-block-call statements, so
// `D.a`/`D.b`/`D.c` silently stayed at their zero default.

function build(): string {
  class D {
    static a = 7;
    static b = 3 * 4;
    static c: number;
    static {
      D.c = D.a + D.b;
    }
  }
  return `${D.a},${D.b},${D.c}`;
}

// Same shape inside an arrow IIFE.
const arrow = (() => {
  class E {
    static v = 0;
    static {
      E.v = 99;
    }
  }
  return E.v;
})();

// Top-level class (already worked) — guards against regressing it.
class T {
  static a = 7;
  static b = 3 * 4;
  static c: number;
  static {
    T.c = T.a + T.b;
  }
}

console.log("in-func:", build()); // 7,12,19
console.log("arrow:", arrow); // 99
console.log("top-level:", `${T.a},${T.b},${T.c}`); // 7,12,19
