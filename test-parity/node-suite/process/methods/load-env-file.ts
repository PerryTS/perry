// process.loadEnvFile is a callable (#1399). The test only probes the
// surface — calling it would mutate process.env (host-specific path).
console.log("is function:", typeof process.loadEnvFile === "function");
