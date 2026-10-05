import { execFile } from "node:child_process";
import { promisify } from "node:util";
import type { SetupInputs } from "./inputs";

const execFileAsync = promisify(execFile);

export async function getPythonRuntimeId(inputs: SetupInputs): Promise<string> {
  if (!inputs.activateEnvironment) {
    return "";
  }

  try {
    // The venv path restricts results to this invocation's runtime, even if
    // earlier setup-uv calls installed other Python versions in the same job.
    const { stdout } = await execFileAsync(
      "uv",
      [
        "python",
        "list",
        inputs.venvPath,
        "--only-installed",
        "--output-format",
        "json",
      ],
      { encoding: "utf8" },
    );
    const pythons = JSON.parse(stdout);
    if (
      !Array.isArray(pythons) ||
      pythons.length !== 1 ||
      typeof pythons[0]?.key !== "string" ||
      pythons[0].key === ""
    ) {
      throw new Error("Expected one installed Python with a runtime key");
    }
    return pythons[0].key;
  } catch (error) {
    throw new Error(
      `Failed to identify the activated environment's Python runtime: ${error instanceof Error ? error.message : String(error)}`,
      { cause: error },
    );
  }
}
