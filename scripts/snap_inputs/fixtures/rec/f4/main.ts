// SNAPREC2 F4: a <-> b under import(): both bodies run before either settles; no cut between a's and b's exits.
await 0;
await import("./a.ts");
gc();
console.log("REC_F4");
