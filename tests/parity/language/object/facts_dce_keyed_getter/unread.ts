function unused() { return 99; }
const o = {}; Object.defineProperty(o, 0, { get: unused }); console.log("unread");
