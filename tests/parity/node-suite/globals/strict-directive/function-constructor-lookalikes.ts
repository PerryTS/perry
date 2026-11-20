// @ts-nocheck

function show(label, value) {
  console.log(label + ":" + String(value));
}

const doubledSpace = Function('"use  strict"; var public = 1; return public;');
const uppercase = Function('"USE STRICT"; var package = 2; return package;');
const escapedSpace = new Function('"use\\x20strict"; var yield = 3; return yield;');
const interrupted = new Function('var interface = 4; "use strict"; return interface;');

show("function doubled-space reserved", doubledSpace());
show("function uppercase reserved", uppercase());
show("new function escaped reserved", escapedSpace());
show("new function interrupted reserved", interrupted());
