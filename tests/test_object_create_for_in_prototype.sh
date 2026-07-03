#!/usr/bin/env bash
source "$(dirname "$0")/_perry_test_lib.sh"

perry_run main.ts <<'TS'
const proto: any = { inherited: 2 };
const input: any = Object.create(proto);
input.own = 1;

const forInKeys: string[] = [];
for (const key in input) {
  forInKeys.push(key);
}

console.log(JSON.stringify({
  protoIdentity: Object.getPrototypeOf(input) === proto,
  protoKeys: Object.keys(Object.getPrototypeOf(input)),
  forInKeys,
  keys: Object.keys(input),
  spread: { ...input },
  assign: Object.assign({}, input),
  ctorIsObject: input.constructor === Object,
}));
TS

perry_expect '{"protoIdentity":true,"protoKeys":["inherited"],"forInKeys":["own","inherited"],"keys":["own"],"spread":{"own":1},"assign":{"own":1},"ctorIsObject":true}'
perry_pass "Object.create for-in prototype chain"
