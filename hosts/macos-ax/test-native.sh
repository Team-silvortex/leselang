#!/bin/sh
# Pure bridge boundary tests: no remote AX calls, permissions or application edits.
set -eu
host_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$host_dir/../.."
mkdir -p target
xcrun clang -fobjc-arc -fobjc-arc-exceptions -Wall -Wextra -Werror \
    -mmacosx-version-min=14.0 hosts/macos-ax/native/bridge_tests.m \
    -framework AppKit -framework ApplicationServices -framework Foundation \
    -o target/leselang-ax-boundary-tests
exec target/leselang-ax-boundary-tests
