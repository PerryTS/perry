function show(label: string, value: unknown) {
  console.log(label + ":", String(value));
}

function showJson(label: string, value: unknown) {
  console.log(label + ":", JSON.stringify(value));
}

for (const ch of ["~", "*", "-", ".", "_", " ", "!", "é"]) {
  const sp = new URLSearchParams();
  sp.set("x", ch);
  showJson(`param ${ch}`, sp.toString());
}

const owner = new URL("https://example.com/?x=~&y=%7E");
show("owner initial href", owner.href);
show("owner params string", owner.searchParams.toString());
owner.searchParams.sort();
show("owner sorted href", owner.href);
