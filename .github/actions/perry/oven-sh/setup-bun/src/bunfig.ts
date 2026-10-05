import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { info } from "@actions/core";
import { parse, stringify } from "@iarna/toml";
import { Registry } from "./registry";

type BunfigConfig = {
  install?: {
    registry?: {
      url: string;
      token?: string;
    };
    scopes?: Record<string, { url: string; token?: string }>;
  };
  [key: string]: any;
};

export function writeBunfig(path: string, registries: Registry[]): void {
  if (!registries.length) {
    return;
  }

  let globalRegistryCount = 0;
  registries.forEach((registry) => {
    try {
      new URL(registry.url);
    } catch {
      throw new Error(`Invalid registry URL: ${registry.url}`);
    }

    if (!registry.scope) {
      globalRegistryCount++;
    }
  });

  if (globalRegistryCount > 1) {
    throw new Error("You can't have more than one global registry.");
  }

  info(`Writing bunfig.toml to '${path}'.`);

  let config: BunfigConfig = {};
  if (existsSync(path)) {
    try {
      const content = readFileSync(path, { encoding: "utf-8" });
      config = parse(content) as BunfigConfig;
    } catch (error) {
      info(`Error reading existing bunfig: ${error.message}`);
      config = {};
    }
  }

  config.install = config?.install || {};
  config.install.scopes = config?.install.scopes || {};

  const globalRegistry = registries.find((r) => !r.scope);
  if (globalRegistry) {
    config.install.registry = {
      url: globalRegistry.url,
      ...(globalRegistry.token ? { token: globalRegistry.token } : {}),
    };
  }

  for (const registry of registries) {
    if (registry.scope) {
      const scopeName = registry.scope.startsWith("@")
        ? registry.scope.toLowerCase()
        : `@${registry.scope.toLowerCase()}`;

      config.install.scopes[scopeName] = {
        url: registry.url,
        ...(registry.token ? { token: registry.token } : {}),
      };
    }
  }

  if (Object.keys(config.install.scopes).length === 0) {
    delete config.install.scopes;
  }

  writeFileSync(path, stringify(config), { encoding: "utf8" });
}
