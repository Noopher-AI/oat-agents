#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 oat-agents contributors
#
# Checks that every source file carries the license header within its first
# three lines. Test fixtures are exempt: they are plugin content kept verbatim.
set -eu

root=$(git rev-parse --show-toplevel)
status=0

for file in $(git -C "$root" ls-files '*.rs' '*.sh' '*.py' ':!tests/fixtures/'); do
    if ! head -n 3 "$root/$file" | grep -q 'SPDX-License-Identifier: MIT'; then
        echo "$file: missing the SPDX-License-Identifier header" >&2
        status=1
    fi
done

[ "$status" -eq 0 ] && echo "license headers: ok"
exit "$status"
