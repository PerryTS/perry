// an imaged dependency whose module-level variable is written by the entry
export let keep: any;
export function hold(v: any) { keep = v; }
