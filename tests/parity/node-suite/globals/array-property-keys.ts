function show(label: string, value: unknown) {
  console.log(label + ":", JSON.stringify(value));
}

const nonInteger: any = [0, 1, 2];
nonInteger[1.5] = "half";
show("noninteger numeric read", nonInteger[1.5]);
show("noninteger string read", nonInteger["1.5"]);
show("index one unchanged", nonInteger[1]);
show("noninteger own", Object.prototype.hasOwnProperty.call(nonInteger, "1.5"));
show("noninteger length", nonInteger.length);
show("delete noninteger", delete nonInteger[1.5]);
show("noninteger after delete", Object.prototype.hasOwnProperty.call(nonInteger, "1.5"));
show("index one after delete", nonInteger[1]);

const numericString: any = [];
numericString["1.5"] = "string half";
show("string noninteger string read", numericString["1.5"]);
show("string noninteger numeric read", numericString[1.5]);
show("string noninteger index one", numericString[1]);
show("string noninteger length", numericString.length);

const bigintKey: any = [];
bigintKey[10n as any] = "ten";
show("bigint key string read", bigintKey["10"]);
show("bigint key numeric read", bigintKey[10]);
show("bigint key own", Object.prototype.hasOwnProperty.call(bigintKey, "10"));
show("bigint key in", "10" in bigintKey);
show("bigint key length", bigintKey.length);
show("delete bigint key", delete bigintKey["10"]);
show("bigint key after delete", bigintKey["10"]);
show("bigint key own after delete", Object.prototype.hasOwnProperty.call(bigintKey, "10"));
show("bigint key length after delete", bigintKey.length);
