# Documentation catalogs

Catalog generation uses the versions in `i18n-toolchain.env`: mdBook 0.5.4,
mdbook-i18n-helpers 0.4.0, and GNU gettext 0.21. The gettext version affects
wrapping, including obsolete messages. Regenerating with another version can
change thousands of lines without changing a translation.

Install mdBook from its matching release or with Cargo, then install the helpers:

```sh
source docs/i18n-toolchain.env
cargo install mdbook --locked --version "$MDBOOK_VERSION"
cargo install mdbook-i18n-helpers --locked --version "$MDBOOK_I18N_VERSION"
```

On [Ubuntu 24.04](https://packages.ubuntu.com/noble/gettext),
`sudo apt-get install gettext` supplies gettext 0.21. On other
systems, install that version into a separate prefix and put its `bin` directory
first on `PATH`. Check `msgmerge --version` before regenerating. In particular,
a newer Homebrew gettext is not interchangeable with the pinned version.
The helper rejects mismatched mdBook, msgmerge, and msginit versions before
running their catalog-writing operations.

Regenerate and validate from the repository root:

```sh
./docs/i18n.sh extract
./docs/i18n.sh sync
for catalog in docs/po/*.po; do
  msgfmt --check --check-format --output-file=/dev/null "$catalog"
done
./docs/i18n.sh build-all
```

Commit `docs/po/messages.pot` and every changed language catalog. To verify
reproducibility after committing, run `extract` and `sync` again, then:

```sh
git diff --exit-code -- docs/po
```

Keep source locations in the catalogs: they let translators find each message
in context. A source edit can legitimately change those references. Toolchain
upgrades should update `i18n-toolchain.env` and regenerate all catalogs in a
separate commit so formatting changes are reviewable.

The deploy workflow checks catalog freshness before publishing on release tags
or manual dispatch. The required lint job exercises the version guards on every
PR. Full catalog freshness is still a release check, not a required PR check;
promoting it should follow a green run of the regenerated catalogs.
