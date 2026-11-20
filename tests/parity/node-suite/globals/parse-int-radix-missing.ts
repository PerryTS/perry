function show(label: string, value: number) {
  console.log(label, Number.isNaN(value) ? "NaN" : value);
}

show("parseInt missing", parseInt());
show("parseFloat missing", parseFloat());
show("parseInt number arg", parseInt(10 as any));
show("parseFloat number arg", parseFloat(3.5 as any));

show("hex explicit 10", parseInt("0x10", 10));
show("hex explicit 16", parseInt("0x10", 16));
show("hex auto", parseInt("0x10", 0));
show("radix 37", parseInt("10", 37));
show("radix -2", parseInt("10", -2));
show("radix Infinity", parseInt("10", Infinity));
show("radix NaN", parseInt("10", NaN));
show("radix string 2", parseInt("10", "2" as any));
show("radix wrap 2", parseInt("10", 4294967298));
show("radix fractional", parseInt("10", 2.9));

show("Number.parseInt missing", Number.parseInt());
show("Number.parseFloat missing", Number.parseFloat());
show("Number hex explicit 10", Number.parseInt("0x10", 10));
show("Number radix Infinity", Number.parseInt("10", Infinity));
