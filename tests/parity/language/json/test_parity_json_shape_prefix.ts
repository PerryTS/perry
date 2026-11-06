for (const source of [
  '[{"a":1,"b":2},{"a":3,"b":4},{"a":5,"a":6,"b":7}]',
  '[{"a":1,"b":2},{"\\u0061":3,"b":4},{"a":5,"b":6,"\\u0061":7}]',
  '[{"a":1,"n":{"x":1},"b":2},{"a":3,"n":{"z":4},"a":5,"b":6}]',
  '[{"a":1,"b":2},{"b":3,"a":4},{"a":5,"b":6,"c":7,"a":8}]',
  '[{"0":1,"a":2},{"0":3,"a":4},{"0":5,"a":6,"0":7}]',
  '[{"é":1,"😀":2},{"é":3,"😀":4},{"é":5,"😀":6,"é":7}]',
  '[{"__proto__":1,"constructor":2},{"__proto__":3,"constructor":4},{"__proto__":5,"constructor":6,"__proto__":7}]'
]) {
  const parsed = JSON.parse(source);
  console.log(JSON.stringify(parsed), parsed.length, parsed[0].missing);
}
console.log(JSON.stringify([-2147483649, -2147483648, -1, -0, 0, 1, 2147483647, 2147483648, 4294967295, 9007199254740991, NaN, Infinity, 1.25, 0.000001, 1e21]));
console.log(JSON.stringify({ get a() { return 17; }, get b() { return [1, null]; } }));
