function show(label: string, value: unknown) {
  console.log(label + ": " + JSON.stringify(value));
}

const input = "user@example";
const positive = /(?<=@)\w+/;
const negative = /(?<!@)\w+/;
const missing = /(?<=#)\w+/;
const build = () => /(?<=@)\w+/;
const dynamicTest = (regex: any, value: string) => regex["test"](value);

show("positive test", positive.test(input));
show("negative test", negative.test(input));
show("missing test", missing.test(input));
show("built test", build().test(input));
show("dynamic test", dynamicTest(build(), input));
show("match control", input.match(positive)?.[0]);
show("exec control", positive.exec(input)?.[0]);
