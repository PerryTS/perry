// process.getActiveResourcesInfo() returns a string[] of names of libuv
// handles keeping the loop alive. Perry returns an empty array (no
// introspection yet). The test verifies the shape, not the contents.
console.log("is array:", Array.isArray(process.getActiveResourcesInfo()));
