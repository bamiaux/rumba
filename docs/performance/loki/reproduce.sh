#!/usr/bin/env bash
set -euo pipefail
repo=$(git rev-parse --show-toplevel)
work=$(mktemp -d /tmp/rumba-loki.XXXXXX)
printf 'Sources, binaries and results: %s\n' "$work"
for variant in before after; do
    mkdir "$work/$variant"
    git -C "$repo" archive HEAD | tar -x -C "$work/$variant"
    cp -a "$repo/core/src/." "$work/$variant/core/src/"
    cp "$repo/docs/performance/loki/probe.rs" "$work/$variant/core/examples/loki_probe.rs"
    cp "$repo/docs/performance/phase3-followup/probe.rs" "$work/$variant/core/examples/phase3_probe.rs"
    cp "$repo/docs/performance/phase3-followup/synthetic.rs" "$work/$variant/core/examples/phase3_synthetic.rs"
done
patch --directory "$work/before" -R -p1 < "$repo/docs/performance/loki/changes.patch"
for variant in before after; do
    CARGO_TARGET_DIR="$work/build-$variant" cargo build --locked --release \
        --manifest-path "$work/$variant/Cargo.toml" -p rumba-core --features parse \
        --example loki_probe --example phase3_probe --example phase3_synthetic
    sha256sum "$work/build-$variant/release/examples/loki_probe"
    "$work/build-$variant/release/examples/phase3_probe" snapshot "$work/$variant.outputs"
    "$work/build-$variant/release/examples/phase3_synthetic" > "$work/$variant.synthetic"
done
cmp "$work/before.outputs" "$work/after.outputs"
cmp "$work/before.synthetic" "$work/after.synthetic"
for scope in loki phase3; do
    for variant in before after after before; do
        taskset -c 2 "$work/build-$variant/release/examples/${scope}_probe" bench \
            | tee -a "$work/$variant-$scope-times.txt"
    done
done
"$work/build-after/release/examples/phase3_probe" quality > "$work/quality.txt"
