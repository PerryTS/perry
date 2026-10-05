import {
  compareVersions,
  satisfies,
  validate,
  validateStrict,
} from "compare-versions";
import { Input } from "./action";
import { getArchitecture, getAvx2, getPlatform, request } from "./utils";

export async function getDownloadUrl(options: Input): Promise<string> {
  const { customUrl } = options;
  if (customUrl) {
    return customUrl;
  }

  return await getSemverDownloadUrl(options);
}

async function getSemverDownloadUrl(options: Input): Promise<string> {
  const { version, os, arch, avx2, profile } = options;
  let tag: string | undefined;

  if (validateStrict(version)) {
    tag = `bun-v${version}`;
  }

  if (!tag) {
    const res = (await (
      await request("https://api.github.com/repos/oven-sh/bun/git/refs/tags", {
        headers: options.token
          ? { "Authorization": `Bearer ${options.token}` }
          : {},
      })
    ).json()) as { ref: string }[];

    let tags = res
      .filter(
        (tag) =>
          tag.ref.startsWith("refs/tags/bun-v") ||
          tag.ref === "refs/tags/canary",
      )
      .map((item) => item.ref.replace(/refs\/tags\/(bun-v)?/g, ""))
      .filter(Boolean);

    tag = tags.find((t) => t === version);
    if (!tag) {
      tags = tags.filter((t) => validate(t)).sort(compareVersions);

      const matchedTag =
        version === "latest" || !version
          ? tags.at(-1)
          : tags.filter((t) => satisfies(t, version)).at(-1);

      if (!matchedTag) {
        throw new Error(`No Bun release found matching version '${version}'`);
      }

      tag = `bun-v${matchedTag}`;
    } else if (validate(tag)) {
      tag = `bun-v${tag}`;
    }
  }

  const resolvedTag = tag ?? version;
  const eversion = encodeURIComponent(resolvedTag);
  const eos = encodeURIComponent(os ?? getPlatform());
  const earch = encodeURIComponent(
    getArchitecture(os ?? getPlatform(), arch ?? process.arch, resolvedTag),
  );
  const eavx2 = encodeURIComponent(
    getAvx2(os ?? getPlatform(), arch ?? process.arch, avx2, resolvedTag) ===
      false
      ? "-baseline"
      : "",
  );
  const eprofile = encodeURIComponent(profile === true ? "-profile" : "");

  const { href } = new URL(
    `${eversion}/bun-${eos}-${earch}${eavx2}${eprofile}.zip`,
    "https://github.com/oven-sh/bun/releases/download/",
  );

  return href;
}
