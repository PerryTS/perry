A labeled `break`/`continue` aimed at a `for...of` loop over an array inside a
generator or async function works again. Such a loop now lowers inside a
`try`/`finally` that releases its iterator record, and the generator transform
did not recognise the label's loop there, so `break label` produced a malformed
iterator result and later loops could hang.
