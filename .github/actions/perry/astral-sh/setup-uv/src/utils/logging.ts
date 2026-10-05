import * as core from "@actions/core";

let quiet: boolean | undefined;

function isQuiet(): boolean {
  if (quiet === undefined) {
    quiet =
      typeof core.getInput === "function" && core.getInput("quiet") === "true";
  }
  return quiet;
}

export function info(msg: string): void {
  if (!isQuiet()) {
    core.info(msg);
  }
}

export const warning = core.warning;
export const error = core.error;
export const debug = core.debug;
