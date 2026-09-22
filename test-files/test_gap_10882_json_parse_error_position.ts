const cases: [string, string][] = [
  ['{\n  "a": 1,\n  "b": 2,,\n}\n', 'position 21 (line 3 column 10)'],
  ['{"🙂":1}x', 'position 8 (line 1 column 9)'],
  ['[01]', 'position 2 (line 1 column 3)'],
  ['"a\\q"', 'position 3 (line 1 column 4)'],
  ['{\r\n"x":1,,}', 'position 9 (line 2 column 7)'],
];

for (const [input, location] of cases) {
  try {
    JSON.parse(input);
    console.log('missing error');
  } catch (error: any) {
    console.log(error.name, error.message.includes(location));
  }
}

try {
  JSON.parse<{ a: number }[]>('[{"a":1},{"a":2,,}]');
  console.log('missing typed error');
} catch (error: any) {
  console.log(error.name, error.message.includes('position 16 (line 1 column 17)'));
}
