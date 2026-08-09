#!/bin/bash
set -euo pipefail

NAME=${1:-World}
RETRIES=3
GREETING="Hello"

log() {
    local level=$1
    shift
    echo "[$level] $*"
}

greet() {
    local name=$1
    echo "${GREETING}, ${name}!"
}

count_words() {
    local text=$1
    echo "$text" | wc -w
}

usage() {
    echo "cli: [name] (defaults to World)"
}

main() {
    local attempt=1
    while [ "$attempt" -le "$RETRIES" ]; do
        case "$NAME" in
            World)
                log info "default target"
                ;;
            *)
                log info "custom target: $NAME"
                ;;
        esac
        attempt=$((attempt + 1))
    done

    for word in alpha beta gamma; do
        log debug "$word"
    done

    greet "$NAME"
    count_words "one two three"
}

main "$@"
