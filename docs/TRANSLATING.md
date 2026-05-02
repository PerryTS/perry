# Translating the Perry docs

The Perry documentation is internationalized via [`mdbook-i18n-helpers`](https://github.com/google/mdbook-i18n-helpers) (gettext `.po` files). English lives in `docs/src/`; translations live in `docs/po/<lang>.po`.

Site layout:

- English: `https://docs.perryts.com/`
- Other languages: `https://docs.perryts.com/<lang>/` (currently `/de/`, `/ja/`, `/ko/`, `/zh-CN/`)

The current `.po` files include a seed translation of the sidebar navigation, the introduction page, and the Getting Started chapter headings. Everything beyond that falls through to English until translators fill in more `msgstr` entries.

## Translating an existing language

1. Open `docs/po/<lang>.po` (e.g. `de.po`).
2. Find entries with `msgstr ""` and fill in the translation. Leave `msgid` (the English source) untouched.
3. Preview locally: `./docs/i18n.sh build de` then open `docs/book/de/introduction.html`.
4. Open a PR. Untranslated entries fall back to English automatically — partial PRs are welcome.

Notes:

- Markdown formatting in the source is preserved by gettext — translate the prose, keep `**bold**`, `[links](...)`, and code spans intact.
- Code blocks are extracted as their own entries; usually leave them as-is unless you're translating an inline comment.
- Entries marked `#, fuzzy` mean the English source changed since the translation was written. Review, fix the `msgstr`, and remove the `#, fuzzy` line.

## Adding a new language

```bash
./docs/i18n.sh add zh-CN     # creates po/zh-CN.po
# edit po/zh-CN.po, fill in some msgstr entries
./docs/i18n.sh build zh-CN   # preview at docs/book/zh-CN/
```

Then add the language to the picker in `docs/theme/language-switcher.js` (one line in the `LANGS` array) and submit a PR. CI will auto-build it; no workflow edit needed.

Use a [BCP 47](https://en.wikipedia.org/wiki/IETF_language_tag) code: `de`, `ja`, `ko`, `zh-CN`, `zh-TW`, `fr`, `es`, `pt-BR`, etc.

## Maintainer: refreshing translations after English changes

When the English source changes, the `.po` files need to be re-synced so translators see the new/changed strings:

```bash
./docs/i18n.sh extract   # regenerates po/messages.pot from src/
./docs/i18n.sh sync      # merges .pot into every po/<lang>.po
```

`sync` preserves existing translations, adds new entries with empty `msgstr`, and marks edited entries `#, fuzzy` for review. Commit the updated `.pot` and `.po` files alongside the English changes.
