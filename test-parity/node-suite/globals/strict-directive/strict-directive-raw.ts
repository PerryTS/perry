// @ts-nocheck
"use\x20strict";

function show(label, value) {
  console.log(label + ":" + String(value));
}

function exactDouble() {
  "use strict";
  return this === undefined;
}

function exactSingle() {
  'use strict';
  return this === undefined;
}

function escapedBody() {
  "use\x20strict";
  return this === globalThis;
}

function trailingBody() {
  "use strict ";
  return this === globalThis;
}

function parenthesizedBody() {
  ("use strict");
  return this === globalThis;
}

function interruptedBody() {
  0;
  "use strict";
  return this === globalThis;
}

function nestedEscaped() {
  function inner() {
    "use\x20strict";
    return this === globalThis;
  }
  return inner();
}

const exprExact = function () {
  "use strict";
  return this === undefined;
};

const exprEscaped = function () {
  "use\x20strict";
  return this === globalThis;
};

const arrowExact = () => {
  "use strict";
  return (function () {
    return this === undefined;
  })();
};

const arrowEscaped = () => {
  "use\x20strict";
  return (function () {
    return this === globalThis;
  })();
};

show("exact double strict", exactDouble());
show("exact single strict", exactSingle());
show("escaped body sloppy", escapedBody());
show("trailing body sloppy", trailingBody());
show("parenthesized body sloppy", parenthesizedBody());
show("interrupted body sloppy", interruptedBody());
show("nested escaped sloppy", nestedEscaped());
show("expr exact strict", exprExact());
show("expr escaped sloppy", exprEscaped());
show("arrow exact strict", arrowExact());
show("arrow escaped sloppy", arrowEscaped());
