#!/usr/bin/env bash
# Translation helper for the Perry docs.
#
# Usage:
#   ./docs/i18n.sh extract         Regenerate po/messages.pot from English source
#   ./docs/i18n.sh sync            Merge .pot into every po/<lang>.po
#   ./docs/i18n.sh add <lang>      Create po/<lang>.po (e.g. ./docs/i18n.sh add zh-CN)
#   ./docs/i18n.sh build <lang>    Build a single language into book/<lang>/
#   ./docs/i18n.sh build-all       Build English + every po/<lang>.po into book/

set -euo pipefail
cd "$(dirname "$0")"

cmd=${1:-help}

require() {
  command -v "$1" >/dev/null 2>&1 || { echo "missing tool: $1" >&2; exit 1; }
}

extract() {
  require mdbook
  require mdbook-xgettext
  rm -rf /tmp/perry-i18n-extract
  MDBOOK_OUTPUT__XGETTEXT__POT_FILE=messages.pot \
    mdbook build -d /tmp/perry-i18n-extract >/dev/null
  mkdir -p po
  cp /tmp/perry-i18n-extract/xgettext/messages.pot po/messages.pot
  rm -rf /tmp/perry-i18n-extract
  echo "wrote po/messages.pot"
}

sync_all() {
  require msgmerge
  for f in po/*.po; do
    [ -e "$f" ] || continue
    msgmerge --quiet --update --backup=none "$f" po/messages.pot
    echo "synced $f"
  done
}

add_lang() {
  require msginit
  local lang=${1:?usage: i18n.sh add <lang>}
  local out="po/${lang}.po"
  if [ -e "$out" ]; then
    echo "$out already exists" >&2
    exit 1
  fi
  [ -e po/messages.pot ] || extract
  msginit -i po/messages.pot -l "${lang}.UTF-8" -o "$out" --no-translator
  echo "created $out — translate msgstr entries and submit a PR"
}

build_one() {
  require mdbook
  require mdbook-gettext
  local lang=${1:?usage: i18n.sh build <lang>}
  if [ "$lang" = "en" ]; then
    mdbook build -d book
  else
    [ -e "po/${lang}.po" ] || { echo "po/${lang}.po not found" >&2; exit 1; }
    MDBOOK_BOOK__LANGUAGE="$lang" mdbook build -d "book/${lang}"
  fi
}

build_all() {
  build_one en
  for f in po/*.po; do
    [ -e "$f" ] || continue
    local lang
    lang=$(basename "$f" .po)
    build_one "$lang"
  done
}

case "$cmd" in
  extract)   extract ;;
  sync)      sync_all ;;
  add)       add_lang "${2:-}" ;;
  build)     build_one "${2:-}" ;;
  build-all) build_all ;;
  *)
    sed -n '2,9p' "$0"
    exit 1 ;;
esac
