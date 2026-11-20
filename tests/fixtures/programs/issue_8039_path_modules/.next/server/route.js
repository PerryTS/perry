globalThis.__issue8039RouteInits = (globalThis.__issue8039RouteInits || 0) + 1;

const root = process.cwd() + "/.next/server/";
const cycle = require(root + "cycle-a.js");
const undefinedValue = require(root + "undefined.js");

exports.handle = async function handle(id) {
    const work = await import("./async-work.js");
    const index = Number(id.slice(id.lastIndexOf("-") + 1));
    return {
        id,
        cycle: cycle.seenByB,
        undefinedIsReal: undefinedValue === undefined,
        checksum: work.checksum(index),
        routeInits: globalThis.__issue8039RouteInits,
        aInits: globalThis.__issue8039AInits,
        bInits: globalThis.__issue8039BInits,
        asyncInits: globalThis.__issue8039AsyncInits,
    };
};
