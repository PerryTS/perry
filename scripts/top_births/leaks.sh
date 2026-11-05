#!/bin/bash
# leaks.sh DIR NAME SOURCE_ROOT: the leak table of one census run.
# DIR holds NAME.dump (PERRY_FACTS_BIRTH_CENSUS=1 PERRY_FACTS_UNITS=0 PERRY_FACTS_DUMP=...) and
# NAME.ledger.tsv.gz (scripts' ledger.py output). Writes the analysis files into DIR
# (NAME.openers.tsv: the first write that opened each native-write gate, openers.py)
# and prints the table (TOTAL, injector_locations, one row per injection kind) to stdout.
set -eu
D=$(cd "$1" && pwd); N=$2; S=${3:-/root/hwp-move/score/p/tssrc/}
H=$(cd "$(dirname "$0")" && pwd)
export PERRY_BIRTH_OUTPUT=$D PERRY_BIRTH_SOURCE_ROOT=$S
python3 "$H/inst.py" "$N" >&2
python3 "$H/an.py" "$N" causal >&2
python3 "$H/openers.py" "$D/$N.dump" > "$D/$N.openers.tsv"
python3 "$H/leaks.py" "$D"
