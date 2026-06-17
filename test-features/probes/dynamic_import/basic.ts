import("./mod.ts")
    .then((mod) => {
        console.log(`dynamic-import:${mod.label(3)}`);
    })
    .catch((error) => {
        console.error(error);
        process.exitCode = 1;
    });
