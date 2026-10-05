import * as tc from "@actions/tool-cache";

export type ParsedVersionSpecifier =
  | {
      kind: "exact";
      normalized: string;
      raw: string;
    }
  | {
      kind: "latest";
      normalized: "latest";
      raw: string;
    }
  | {
      isSimpleMinimumVersionSpecifier: boolean;
      kind: "range";
      normalized: string;
      raw: string;
    };

export function normalizeVersionSpecifier(specifier: string): string {
  const trimmedSpecifier = specifier.trim();

  if (trimmedSpecifier.startsWith("==")) {
    return trimmedSpecifier.slice(2);
  }

  return trimmedSpecifier;
}

export function parseVersionSpecifier(
  specifier: string,
): ParsedVersionSpecifier {
  const raw = specifier.trim();
  const normalized = normalizeVersionSpecifier(raw);

  if (normalized === "latest") {
    return {
      kind: "latest",
      normalized: "latest",
      raw,
    };
  }

  if (tc.isExplicitVersion(normalized)) {
    return {
      kind: "exact",
      normalized,
      raw,
    };
  }

  return {
    isSimpleMinimumVersionSpecifier: raw.includes(">") && !raw.includes(","),
    kind: "range",
    normalized,
    raw,
  };
}
