// The runtime's own "the builtin namespace" questions read the realm's
// original namespace object: the reflective prototype of a Temporal value
// (`Object.getPrototypeOf(date) === Temporal.PlainDate.prototype`).
// Replacing the global binding first (`globalThis.Temporal = X`) does not
// change it: node never reads the global there.
const g: any = globalThis;
const T = g.Temporal;
const date = T.PlainDate.from("2020-01-02");
const duration = T.Duration.from({ hours: 2 });
const fakeProto = { fake: true };
g.Temporal = { PlainDate: { prototype: fakeProto }, Duration: { prototype: fakeProto } };
console.log("date proto is birth", Object.getPrototypeOf(date) === T.PlainDate.prototype);
console.log("duration proto is birth", Object.getPrototypeOf(duration) === T.Duration.prototype);
console.log("date fake", (Object.getPrototypeOf(date) as any).fake === true);

