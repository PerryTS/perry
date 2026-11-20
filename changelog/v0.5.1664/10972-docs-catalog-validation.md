### Fixed

- Docs checks now validate the committed gettext catalogs and build every translation without regenerating catalogs during CI. Translation refreshes remain a reviewed change, avoiding a permanently failing freshness diff and unintended loss of existing translations. (#10955)
