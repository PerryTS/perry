Read WTF-8 string positions directly as bytes for `startsWith` and `endsWith`, avoiding an unchecked UTF-8 borrow when a string contains a lone surrogate.
