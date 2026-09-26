#!/bin/sh
# Checks the invariants of .dev_docs that nothing else guards:
# every ADR-NNNN citation resolves to a record, and every record is listed in
# the index in .dev_docs/adr/README.md.
set -eu

root=$(git rev-parse --show-toplevel)
adr_dir="$root/.dev_docs/adr"
index="$adr_dir/README.md"
status=0

existing=$(find "$adr_dir" -maxdepth 1 -name '[0-9][0-9][0-9][0-9]-*.md' -exec basename {} \; \
    | cut -c1-4 | sort -u)

for number in $existing; do
    count=$(find "$adr_dir" -maxdepth 1 -name "$number-*.md" | wc -l)
    if [ "$count" -ne 1 ]; then
        echo "ADR-$number: $count files share this number" >&2
        status=1
    fi
    if ! grep -q "^| ADR-$number |" "$index"; then
        echo "ADR-$number: missing from the index in .dev_docs/adr/README.md" >&2
        status=1
    fi
done

cited=$(git -C "$root" grep -ohE 'ADR-[0-9]{4}' | cut -c5-8 | sort -u)
for number in $cited; do
    if ! printf '%s\n' "$existing" | grep -qx "$number"; then
        where=$(git -C "$root" grep -lE "ADR-$number" | tr '\n' ' ')
        echo "ADR-$number: cited in $where but no such record exists" >&2
        status=1
    fi
done

[ "$status" -eq 0 ] && echo "dev docs: ok"
exit "$status"
