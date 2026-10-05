interface VariantAwareEntry {
  variant?: string;
}

export function selectDefaultVariant<T extends VariantAwareEntry>(
  entries: T[],
  duplicateEntryDescription: string,
): T {
  const firstEntry = entries[0];
  if (firstEntry === undefined) {
    throw new Error("selectDefaultVariant requires at least one candidate.");
  }

  if (entries.length === 1) {
    return firstEntry;
  }

  const defaultEntries = entries.filter((entry) =>
    isDefaultVariant(entry.variant),
  );
  if (defaultEntries.length === 1) {
    return defaultEntries[0];
  }

  throw new Error(
    `${duplicateEntryDescription} with variants ${formatVariants(entries)}. setup-uv currently requires a single default variant for duplicate platform entries.`,
  );
}

function isDefaultVariant(variant: string | undefined): boolean {
  return variant === undefined || variant === "default";
}

function formatVariants<T extends VariantAwareEntry>(entries: T[]): string {
  return entries
    .map((entry) => entry.variant ?? "default")
    .sort((left, right) => left.localeCompare(right))
    .join(", ");
}
