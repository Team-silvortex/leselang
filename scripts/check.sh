#!/bin/sh
# Shared local/CI checks. Dependency installation and interactive GUI runs stay explicit.
set -eu

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_dir"

usage() {
    cat <<'EOF'
Usage: sh scripts/check.sh GROUP
       sh scripts/check.sh --list [GROUP]

Groups:
  tensor          Development tensor validation, query tests and strict Clippy
  language        Language workspace: format, all-feature tests, strict Clippy
  consumer        Independent Rust Consumer and catalog agent
  scalar-consumers  Compare Python, C11 and C++17 hosts against the real worker (requires libcjson)
  javascript      SDK (including packed Consumer) and Tauri frontend tests
  registered      Standalone registered-object host, worker and Rust Consumer
  registered-gui  Compare registered GUI observations from Rust, Node and C# Consumers
  benchmark-registered-gui  Opt-in Release benchmark [--worker-phases] [--warmup N] [--samples N]
  node-consumer   Portable Node carrier, failure tests, real VM Consumer, Tauri plan
  dotnet-consumer C# application against the real worker and shared contract (.NET 10 SDK)
  appkit          AppKit declarations and non-interactive host checks
  macos-ax        AX fake-backend checks; native boundary checks on macOS
  tauri-portable  Tauri without desktop features
  tauri-desktop   macOS desktop transport tests/build; does not launch a GUI
  smoke           CLI/embedding examples and offline AppKit/Calculator plans
  all             All applicable groups, sequentially

Install pinned dependencies first; Cargo uses --locked --offline. Checks use
the lean profile. The optional benchmark uses Release and runs separately from all.
--list prints commands without running them or creating output directories;
with no group it lists all checks applicable to this platform.
smoke expects appkit and macos-ax builds; all runs these groups first.
EOF
}

list_only=false
if [ "${1:-}" = "--list" ]; then
    list_only=true
    shift
    if [ "$#" -eq 0 ]; then set -- all; fi
fi
if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
    usage
    exit 0
fi
if [ "$#" -lt 1 ] || { [ "$1" != benchmark-registered-gui ] && [ "$#" -ne 1 ]; }; then
    usage >&2
    exit 2
fi

run() {
    printf '+ '
    printf '%s ' "$@"
    printf '\n'
    if [ "$list_only" = false ]; then "$@"; fi
}

cargo_checked() {
    action=$1
    shift
    run cargo "$action" --locked --offline --profile lean --target-dir "$repo_dir/target" "$@"
}

format() {
    run cargo fmt "$@" --all -- --check
}

plan_directory() {
    if [ "$list_only" = true ]; then
        plan_root='target/verification/<new-run>'
    else
        mkdir -p target/verification
        plan_root=$(mktemp -d "$repo_dir/target/verification/plans.XXXXXX")
        printf 'Offline plan evidence: %s\n' "$plan_root"
    fi
}

build_dotnet_consumer() {
    dotnet_repo="$repo_dir"
    exe_suffix=
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*)
            dotnet_repo=$(cygpath -m "$repo_dir")
            exe_suffix=.exe
            ;;
    esac
    artifacts="$dotnet_repo/target/dotnet"
    export DOTNET_CLI_HOME="$artifacts/home" DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_GENERATE_ASPNET_CERTIFICATE=false
    run dotnet restore consumer/dotnet/LeseLang.Consumer.csproj --artifacts-path "$artifacts" \
        --source "$dotnet_repo/packages/dotnet" --disable-build-servers -p:NuGetAudit=false
    run dotnet build consumer/dotnet/LeseLang.Consumer.csproj --configuration "${1:-Debug}" --artifacts-path "$artifacts" \
        --no-restore --disable-build-servers
}

check_group() {
    case "$1" in
        tensor)
            manifest=tools/leselang-dev/Cargo.toml
            format --manifest-path "$manifest"
            cargo_checked run --manifest-path "$manifest" -- tensor validate
            cargo_checked test --manifest-path "$manifest"
            cargo_checked clippy --manifest-path "$manifest" --all-targets -- -D warnings
            ;;
        language)
            format
            cargo_checked test --workspace --all-features
            cargo_checked clippy --workspace --all-features --all-targets -- -D warnings
            ;;
        consumer)
            format --manifest-path consumer/Cargo.toml
            cargo_checked test --manifest-path consumer/Cargo.toml
            cargo_checked test --manifest-path consumer/Cargo.toml --example catalog_agent
            cargo_checked clippy --manifest-path consumer/Cargo.toml --all-targets -- -D warnings
            ;;
        scalar-consumers)
            cargo_checked build --manifest-path hosts/registered/Cargo.toml --bin leselang-agent-worker
            run python3 -B -m unittest discover -s consumer/python
            run python3 scripts/check-scalar-consumers.py "$repo_dir/target/lean/leselang-agent-worker"
            ;;
        javascript)
            run npm test --prefix packages/javascript
            run npm test --prefix hosts/tauri
            ;;
        registered)
            format --manifest-path hosts/registered/Cargo.toml
            format --manifest-path hosts/registered/tests/consumer/Cargo.toml
            cargo_checked test --manifest-path hosts/registered/Cargo.toml
            cargo_checked clippy --manifest-path hosts/registered/Cargo.toml --all-targets -- -D warnings
            cargo_checked build --manifest-path hosts/registered/Cargo.toml --bin leselang-agent-worker
            cargo_checked run --manifest-path hosts/registered/tests/consumer/Cargo.toml
            cargo_checked clippy --manifest-path hosts/registered/tests/consumer/Cargo.toml -- -D warnings
            ;;
        registered-gui)
            cargo_checked build --manifest-path hosts/registered/Cargo.toml --bin leselang-agent-worker
            cargo_checked build --manifest-path hosts/registered/tests/consumer/Cargo.toml
            run npm run build --prefix packages/javascript
            build_dotnet_consumer
            run node scripts/compare-registered-gui.mjs
            ;;
        benchmark-registered-gui)
            shift
            run cargo build --release --locked --offline --target-dir "$repo_dir/target" --manifest-path hosts/registered/Cargo.toml --bin leselang-agent-worker
            run cargo build --release --locked --offline --target-dir "$repo_dir/target" --manifest-path hosts/registered/tests/consumer/Cargo.toml
            run npm run build --prefix packages/javascript
            build_dotnet_consumer Release
            run node benchmarks/registered-gui.mjs "$@"
            ;;
        node-consumer)
            run node --test benchmarks/registered-gui.test.mjs benchmarks/compare-registered-gui.test.mjs benchmarks/worker-phases.test.mjs
            cargo_checked build --manifest-path hosts/registered/Cargo.toml --bin leselang-agent-worker
            cargo_checked build --manifest-path consumer/Cargo.toml --example lost_reply_relay
            cargo_checked build --manifest-path hosts/tauri/Cargo.toml --no-default-features --example node_host
            cargo_checked test --manifest-path hosts/tauri/Cargo.toml --no-default-features --example node_host
            cargo_checked build --manifest-path hosts/tauri/Cargo.toml --no-default-features --example agent_worker
            run npm test --prefix consumer/javascript
            cargo_checked test --manifest-path hosts/tauri/Cargo.toml --no-default-features --example node_host -- --ignored
            cargo_checked build --manifest-path hosts/tauri/Cargo.toml --no-default-features
            plan_directory
            cargo_checked run --manifest-path consumer/Cargo.toml --example catalog_agent -- \
                --host target/lean/leselang-tauri-host --output "$plan_root/tauri" --text 'TypeScript agent CI'
            ;;
        dotnet-consumer)
            cargo_checked test --manifest-path hosts/registered/Cargo.toml --lib worker::
            cargo_checked build --manifest-path hosts/registered/Cargo.toml --bin leselang-agent-worker
            cargo_checked build --manifest-path consumer/Cargo.toml --example lost_reply_relay
            build_dotnet_consumer
            run "$artifacts/bin/LeseLang.Consumer/debug/LeseLang.Consumer$exe_suffix" \
                "$dotnet_repo/target/lean/leselang-agent-worker$exe_suffix" "$dotnet_repo/fixtures/application-operations/two-effects.json" \
                "$dotnet_repo/target/lean/examples/lost_reply_relay$exe_suffix"
            run "$artifacts/bin/LeseLang.Consumer/debug/LeseLang.Consumer$exe_suffix" --registered-gui \
                "$dotnet_repo/target/lean/leselang-agent-worker$exe_suffix" "$dotnet_repo/fixtures/registered-gui/publication.json"
            ;;
        appkit)
            format --manifest-path hosts/appkit/Cargo.toml
            cargo_checked test --manifest-path hosts/appkit/Cargo.toml
            cargo_checked clippy --manifest-path hosts/appkit/Cargo.toml --all-targets -- -D warnings
            cargo_checked build --manifest-path hosts/appkit/Cargo.toml
            ;;
        macos-ax)
            format --manifest-path hosts/macos-ax/Cargo.toml
            cargo_checked test --manifest-path hosts/macos-ax/Cargo.toml
            cargo_checked clippy --manifest-path hosts/macos-ax/Cargo.toml --all-targets -- -D warnings
            cargo_checked build --manifest-path hosts/macos-ax/Cargo.toml
            if [ "$(uname -s)" = Darwin ]; then
                run sh hosts/macos-ax/test-native.sh
            else
                printf 'Skipped macOS-only AX native boundary checks.\n'
            fi
            ;;
        tauri-portable)
            format --manifest-path hosts/tauri/Cargo.toml
            cargo_checked test --manifest-path hosts/tauri/Cargo.toml --no-default-features
            cargo_checked clippy --manifest-path hosts/tauri/Cargo.toml --no-default-features --all-targets -- -D warnings
            ;;
        tauri-desktop)
            if [ "$(uname -s)" != Darwin ] && [ "$list_only" = false ]; then
                printf 'tauri-desktop requires macOS.\n' >&2
                return 2
            fi
            cargo_checked build --manifest-path hosts/tauri/Cargo.toml
            cargo_checked test --manifest-path hosts/tauri/Cargo.toml
            cargo_checked clippy --manifest-path hosts/tauri/Cargo.toml --all-targets -- -D warnings
            ;;
        smoke)
            for command in describe 'check' run; do
                if [ "$command" = describe ]; then
                    cargo_checked run -p leselang-vm --bin leselang -- describe
                else
                    cargo_checked run -p leselang-vm --bin leselang -- "$command" examples/arithmetic.lese
                fi
            done
            cargo_checked run -p leselang-vm --bin leselang -- run examples/two_effects.lese --allow stdio
            for entry in preview report; do
                cargo_checked run -p leselang-vm --bin leselang -- run examples/entry_points.lese --entry "$entry" --allow stdio
            done
            for example in embedded_counter protocol_hosts; do
                cargo_checked run --example "$example"
                cargo_checked run --manifest-path consumer/Cargo.toml --example "$example"
            done
            cargo_checked test --example native_counter
            cargo_checked test --manifest-path consumer/Cargo.toml --example native_counter
            cargo_checked run --example native_counter
            cargo_checked run --manifest-path consumer/Cargo.toml --example native_counter
            for mode in --cancel --queued --queued-cancel-pending --queued-cancel-reply; do
                cargo_checked run --example native_counter -- "$mode"
                cargo_checked run --manifest-path consumer/Cargo.toml --example native_counter -- "$mode"
            done
            plan_directory
            cargo_checked run --manifest-path consumer/Cargo.toml --example catalog_agent -- \
                --host target/lean/leselang-appkit-host --output "$plan_root/appkit" --text 'LeseLang agent CI'
            cargo_checked run --manifest-path consumer/Cargo.toml --example catalog_agent -- \
                --host target/lean/leselang-macos-ax-host --host-arg --calculator \
                --task calculate_one_plus_two --output "$plan_root/calculator"
            ;;
        all)
            for group in tensor language consumer appkit macos-ax javascript registered tauri-portable node-consumer dotnet-consumer; do
                check_group "$group"
            done
            case "$(uname -s)" in
                Darwin|Linux) check_group scalar-consumers ;;
            esac
            run node scripts/compare-registered-gui.mjs
            if [ "$(uname -s)" = Darwin ]; then
                check_group tauri-desktop
            else
                printf 'Skipped macOS-only Tauri desktop checks.\n'
            fi
            check_group smoke
            ;;
        *)
            printf 'Unknown check group: %s\n' "$1" >&2
            usage >&2
            return 2
            ;;
    esac
}

check_group "$@"
