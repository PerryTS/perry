// An imaged module reads the dynamic process["argv0"] form (the static forms lower to process.argv[0]).
export const n = (process as any)["argv0"].length;
