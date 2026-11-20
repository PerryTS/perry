async function request(id) {
    const routePath = process.cwd() + "/.next/server/route.js";
    const route = require(routePath);
    return await route.handle(id);
}

async function verifyPass(label) {
    // This exercises same-event-loop re-entrancy and once-only initialization.
    // OS-thread waiters and logical loader ownership are covered by the
    // PathModuleRegistry tests in perry-runtime/src/module_require.rs.
    const values = await Promise.all(
        Array.from({ length: 20 }, (_, index) => request(label + "-" + index)),
    );
    for (let index = 0; index < values.length; index += 1) {
        const value = values[index];
        const expectedId = label + "-" + index;
        if (value.id !== expectedId) throw new Error("lost id " + expectedId);
        if (value.cycle !== "a-partial") throw new Error("bad cycle " + value.cycle);
        if (!value.undefinedIsReal) throw new Error("undefined export became a miss");
        if (value.checksum !== index * 17 + 5) throw new Error("bad checksum " + expectedId);
        if (
            value.routeInits !== 1 ||
            value.aInits !== 1 ||
            value.bInits !== 1 ||
            value.asyncInits !== 1
        ) {
            throw new Error("duplicate init " + JSON.stringify(value));
        }
    }
}

async function main() {
    await verifyPass("cold");
    await verifyPass("warm");
    console.log("PASS: issue 8039 cold/warm path modules");
}

main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});

module.exports = {};
