import { promises as fs } from "node:fs";

export interface ChecksumEntry {
  key: string;
  checksum: string;
}

export async function updateChecksums(
  filePath: string,
  checksumEntries: ChecksumEntry[],
): Promise<void> {
  const deduplicatedEntries = new Map<string, string>();

  for (const entry of checksumEntries) {
    if (deduplicatedEntries.has(entry.key)) {
      continue;
    }

    deduplicatedEntries.set(entry.key, entry.checksum);
  }

  const content = `${JSON.stringify(Object.fromEntries(deduplicatedEntries), null, 2)}\n`;

  await fs.writeFile(filePath, content);
}
