// Two-phase realm negative control: the first touch of the realm is a class
// used as a value (phase A alone), then a read of a phase-B name.
class Core { static n = 1; }
const classes = [Core];
console.log("core", classes[0].n);
console.log("rest", typeof Intl, typeof WeakMap);
