### Performance

- **Required text after a character-class repeat no longer makes each RegExp search cost its start position** (#10201). A pattern such as `/([a-z]+)([0-9]+) /g` or `/[a-z]+[0-9]+ /y` made every `exec` or `test` call over an ASCII string pay work proportional to `lastIndex`, so a loop over one string was quadratic: 21 s for 40,000 matches over a 200,000-character string. The same loop now takes 85 ms. Requires `perex` 0.1.4.
