// cli.ts — perry-styling CLI entry point
//
// Usage:
//   perry-styling generate --tokens tokens.json --out src/theme.ts
//
// Compile to native binary:
//   perry compile src/cli.ts -o dist/cli

import * as fs from "fs";
import { parseTokens } from "./tokens";
import { generateTheme } from "./generator";

// Parse process.argv for named flags
// Returns the value after the given flag, or null if not found
function getArg(flag: string): string | null {
  const args = process.argv;
  for (let i = 2; i < args.length - 1; i = i + 1) {
    if (args[i] === flag) {
      return args[i + 1];
    }
  }
  return null;
}

// Check if a flag exists (boolean flags)
function hasFlag(flag: string): boolean {
  const args = process.argv;
  for (let i = 2; i < args.length; i = i + 1) {
    if (args[i] === flag) {
      return true;
    }
  }
  return false;
}

// Print usage and exit
function printUsage(): void {
  console.log("Usage: perry-styling <command> [options]");
  console.log("");
  console.log("Commands:");
  console.log("  generate    Generate a typed theme.ts from a design token file");
  console.log("");
  console.log("Options for generate:");
  console.log("  --tokens <path>   Path to the JSON token file (required)");
  console.log("  --out    <path>   Path to the output theme.ts file (required)");
  console.log("");
  console.log("Example:");
  console.log("  perry-styling generate --tokens tokens.json --out src/theme.ts");
}

// Main entry point
const args = process.argv;

if (args.length < 3 || args[2] === "--help" || args[2] === "-h") {
  printUsage();
} else if (args[2] === "generate") {
  const tokensPath = getArg("--tokens");
  const outPath = getArg("--out");

  if (tokensPath === null) {
    console.log("Error: --tokens <path> is required");
    printUsage();
  } else if (outPath === null) {
    console.log("Error: --out <path> is required");
    printUsage();
  } else {
    // Read the tokens file
    let jsonContent = "";
    try {
      jsonContent = fs.readFileSync(tokensPath);
    } catch (e) {
      console.log("Error: Could not read tokens file: " + tokensPath);
    }

    if (jsonContent !== "") {
      // Parse tokens
      let tokens = parseTokens(jsonContent);

      // Collect original color values for source comments in the output
      const originalLight: { [key: string]: string } = {};
      const originalDark: { [key: string]: string } = {};
      try {
        const rawObj = JSON.parse(jsonContent);
        if (rawObj !== null && typeof rawObj === "object") {
          const colors = rawObj["colors"];
          if (colors !== null && typeof colors === "object") {
            const colorKeyList = Object.keys(colors);
            for (let i = 0; i < colorKeyList.length; i = i + 1) {
              const k = colorKeyList[i];
              const v = colors[k];
              if (typeof v === "string") {
                if (k.endsWith("-dark")) {
                  originalDark[k] = v;
                } else {
                  originalLight[k] = v;
                }
              }
            }
          }
        }
      } catch (e) {
        // Non-fatal: comments will just be empty
      }

      // Generate the theme.ts content
      const output = generateTheme(tokens, originalLight, originalDark);

      // Write to output path
      try {
        fs.writeFileSync(outPath, output);
        console.log("Generated " + outPath);
        const colorCount = tokens.colorKeys.length;
        const spacingCount = Object.keys(tokens.spacing).length;
        const radiusCount = Object.keys(tokens.radius).length;
        const fontSizeCount = Object.keys(tokens.fontSize).length;
        const borderWidthCount = Object.keys(tokens.borderWidth).length;
        console.log("  " + String(colorCount) + " color(s), " + String(spacingCount) + " spacing token(s), " + String(radiusCount) + " radius token(s), " + String(fontSizeCount) + " fontSize token(s), " + String(borderWidthCount) + " borderWidth token(s)");
      } catch (e) {
        console.log("Error: Could not write output file: " + outPath);
      }
    }
  }
} else {
  console.log("Error: Unknown command: " + args[2]);
  printUsage();
}
