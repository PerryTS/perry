stream/promises pipeline() now treats a trailing options object ({ signal, end }) as options instead of a stage, so a response piped with options is delivered.
