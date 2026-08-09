#!/bin/sh
set -eu

if [ "$#" -eq 0 ]; then
    echo "usage: $0 <extra-participants: 1-9> [dev-runner options]" >&2
    exit 2
fi

participant_count=$1
shift
core_directory=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$core_directory"
exec task dev_runner -- camera-clones --participants "$participant_count" "$@"
