#!/usr/bin/env bash
set -euo pipefail
repo=$(git rev-parse --show-toplevel)
work=$(mktemp -d /tmp/rumba-followup.XXXXXX)
base=e1f9f531e2c5a73bf1bd746cbb25931fd5d3c4b9
cpu=2
printf 'Results and isolated builds: %s\n' "$work"
mkdir "$work/base" "$work/final"
git -C "$repo" archive "$base" | tar -x -C "$work/base"
cp -a "$work/base/." "$work/final/"
cp -a "$repo/core/src/." "$work/final/core/src/"
for variant in base final; do
    cp "$repo/docs/performance/phase3-followup/probe.rs" "$work/$variant/core/examples/phase3_probe.rs"
    cp "$repo/docs/performance/phase3-followup/synthetic.rs" "$work/$variant/core/examples/phase3_synthetic.rs"
    CARGO_TARGET_DIR="$work/build-$variant" cargo build --locked --release \
        --manifest-path "$work/$variant/Cargo.toml" -p rumba-core --features parse \
        --example phase3_probe --example phase3_synthetic
    cp "$work/build-$variant/release/examples/phase3_probe" "$work/$variant-probe"
    sha256sum "$work/$variant-probe"
    taskset -c "$cpu" "$work/$variant-probe" snapshot "$work/$variant.outputs"
    taskset -c "$cpu" "$work/build-$variant/release/examples/phase3_synthetic" > "$work/$variant.synthetic"
done
cmp "$work/base.outputs" "$work/final.outputs"
cmp "$work/base.synthetic" "$work/final.synthetic"
for variant in base final final base; do
    taskset -c "$cpu" "$work/$variant-probe" bench | tee -a "$work/$variant-times.txt"
done
for variant in base final; do
    taskset -c "$cpu" "$work/$variant-probe" quality | tee "$work/$variant-quality.txt"
done
