build:
    cargo build --all-features

release:
    cargo build --release --all-features

wasm:
    cd bindings/wasm && RUSTFLAGS='--cfg getrandom_backend="wasm_js"' wasm-pack build --target web

wasm-dev:
    cd bindings/wasm && RUSTFLAGS='--cfg getrandom_backend="wasm_js"' wasm-pack build --target web --dev --out-dir ../../mba-sandbox/src/wasm

python:
    cd bindings/python && maturin develop --uv --features "jit parse" --release

rumba *ARGS:
    cargo run --bin rumba -- {{ARGS}}

test:
    cargo test datasets --release --all-features -- --nocapture

all-test:
    cargo test --all-features -- --nocapture

# Reproduce the bitwise-frontier baseline and the complete dataset corpus
bitwise-frontier-baseline:
    scripts/test_bitwise_frontier.sh

# Classify hidden-atom dependencies in the five QSynth regressions
hidden-atom-diagnostics *ARGS:
    cargo run -p rumba-core --release --all-features --example hidden_atom_diagnostics -- {{ARGS}}

# Export the 101 historical rewrites and validate them with external Z3
m2-z3 z3_bin='z3':
    nice -n 15 cargo run --release -q -p rumba-core --features parse --example m2_z3_export
    nice -n 15 cargo run --release -q -p rumba-core --features parse --example m2_structured_probe
    nice -n 15 bash scripts/validate_m2_z3.sh {{z3_bin}}

bench:
    cargo bench --all-features

gen-c-headers:
    cd bindings/c && cbindgen --config cbindgen.toml --output include/rumba.h

package-c:
    bindings/c/package.sh
