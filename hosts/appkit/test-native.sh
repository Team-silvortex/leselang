#!/bin/sh
# Explicit real-GUI native bridge tests. No worker-thread or headless fallback.
set -eu
host_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$host_dir/../.."
mkdir -p target
xcrun clang -fobjc-arc -fobjc-arc-exceptions -Wall -Wextra -Werror \
    -mmacosx-version-min=14.0 hosts/appkit/native/bridge_tests.m \
    -framework AppKit -framework Foundation -o target/leselang-appkit-native-tests
exec target/leselang-appkit-native-tests
