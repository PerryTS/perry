export type Registry = {
  url: string;
  scope: string;
  token?: string;
};

/**
 * Parse registries from the simplified format:
 * - Default registry: https://registry.npmjs.org/
 * - Default registry with token: https://registry.npmjs.org/|token123
 * - With scope and credentials in URL: @myorg:https://username:password@registry.myorg.com/
 * - With scope and separate token: @partner:https://registry.partner.com/|basic_token
 */
export function parseRegistries(input: string): Registry[] {
  if (!input?.trim()) return [];

  return input
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map(parseLine)
    .filter(Boolean) as Registry[];
}

function parseLine(line: string): Registry | null {
  const scopeMatch = line.match(
    /^(@[a-z0-9-_.]+|[a-z0-9-_.]+(?=:[a-z]+:\/\/)):(.+)$/i,
  );

  if (scopeMatch) {
    const scope = scopeMatch[1];
    const urlPart = scopeMatch[2].trim();

    const [url, token] = urlPart.split("|", 2).map((p) => p?.trim());

    try {
      new URL(url);

      return {
        url,
        scope,
        ...(token && { token }),
      };
    } catch (e) {
      throw new Error(`Invalid URL in registry configuration: ${url}`);
    }
  } else {
    const [url, token] = line.split("|", 2).map((p) => p?.trim());

    try {
      new URL(url);

      return {
        url,
        scope: "",
        ...(token && { token }),
      };
    } catch (e) {
      throw new Error(`Invalid URL in registry configuration: ${url}`);
    }
  }
}
