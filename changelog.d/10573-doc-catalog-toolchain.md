Pin documentation catalog generation to mdBook 0.5.4, mdbook-i18n-helpers 0.4.0, and GNU gettext 0.21. The deploy workflow uses Ubuntu 24.04 and reads the same version file as the local helper. Catalog-writing commands reject mismatched tool versions before changing output.

Regenerate the template and all language catalogs with that toolchain. Add contributor regeneration instructions and required-lint tests for rejected and accepted versions. Catalog freshness remains checked before release documentation deployment.
