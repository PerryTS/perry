#!/usr/bin/env bash
source "$(dirname "$0")/_perry_test_lib.sh"

perry_run main.ts <<'TS'
const url = new URL("https://example.com/path?q=1");
const C = URL;

console.log(JSON.stringify({
  direct: url instanceof URL,
  dynamic: url instanceof C,
  hasInstance: Function.prototype[Symbol.hasInstance].call(URL, url),
  protoIs: Object.getPrototypeOf(url) === URL.prototype,
  ctorName: (url as any).constructor?.name,
  tag: (url as any)[Symbol.toStringTag],
  objectTag: Object.prototype.toString.call(url),
  protocol: url.protocol,
}));
TS

perry_expect_node
perry_pass "URL instanceof reflection"
