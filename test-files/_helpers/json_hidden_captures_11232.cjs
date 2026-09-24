"use strict";
const K = { tag: "K" };
const cyclic = {};
cyclic.self = cyclic;
class Plain {
  constructor(a) {
    this.a = a;
    this.t = K.tag;
    this.__perry_cap_user = "public";
  }
}
class Cyclic {
  constructor() { this.ok = true; }
  captured() { return cyclic; }
}
module.exports = { Plain, Cyclic };
