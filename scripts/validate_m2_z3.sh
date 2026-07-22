#!/usr/bin/env bash
set -euo pipefail

z3_bin="${1:-z3}"
artifact_dir="${2:-artifacts/m2_z3}"
# Each fallback case uses a three-tactic Z3 portfolio. Four workers therefore
# cap the default run at roughly twelve active solver activities.
parallel_jobs="${M2_Z3_JOBS:-4}"
reuse="${M2_Z3_REUSE:-0}"

if [[ ! -x "$z3_bin" ]] && ! command -v "$z3_bin" >/dev/null 2>&1; then
    echo "Z3 binary not found or not executable: $z3_bin" >&2
    exit 2
fi

for input in \
    required_rewrites.smt2 \
    diagnostic_best.smt2 \
    local_edges.smt2 \
    local_edge_bits.smt2 \
    required_rewrite_bits.smt2 \
    diagnostic_best_bits.smt2 \
    cases.tsv \
    local_edges.tsv \
    rewrite_comparison.tsv \
    structured_lemmas.smt2 \
    structured_bridges.smt2 \
    structured_lemma_bits.smt2 \
    structured_bridge_bits.smt2 \
    structured_cases.tsv \
    structured_export_summary.txt
do
    if [[ ! -f "$artifact_dir/$input" ]]; then
        echo "Missing M2 artifact: $artifact_dir/$input" >&2
        exit 2
    fi
done

"$z3_bin" -version > "$artifact_dir/z3_version.txt"

result_count() {
    grep -Ec '^(unsat|sat|unknown)$' "$1" 2>/dev/null || true
}

run_query_cases() (
    local input="$1"
    local output="$2"
    local expected="$3"
    if [[ "$reuse" == 1 && "$(result_count "$output")" == "$expected" ]]; then
        return
    fi
    local work_dir
    work_dir="$(mktemp -d)"
    trap 'rm -rf -- "$work_dir"' EXIT
    awk -v work_dir="$work_dir" '
        /^\(echo "/ {
            if (id != "") {
                file=work_dir "/" id ".smt2"
                printf "%s%s", header, block > file
                close(file)
            }
            id=$0
            sub(/^\(echo "/, "", id)
            sub(/"\)$/, "", id)
            block=$0 ORS
            next
        }
        id == "" { header=header $0 ORS; next }
        { block=block $0 ORS }
        END {
            if (id != "") {
                file=work_dir "/" id ".smt2"
                printf "%s%s", header, block > file
                close(file)
            }
        }
    ' "$input"
    export z3_bin
    find "$work_dir" -name '*.smt2' -print0 \
        | xargs -0 -r -P "$parallel_jobs" -I '{}' \
            bash -c '"$z3_bin" -smt2 "$1" > "$1.out"' _ '{}'
    find "$work_dir" -name '*.out' -print0 \
        | sort -z \
        | xargs -0 -r cat > "$output"
)

run_query_cases \
    "$artifact_dir/required_rewrites.smt2" \
    "$artifact_dir/required_rewrites.z3.out" \
    101
run_query_cases \
    "$artifact_dir/diagnostic_best.smt2" \
    "$artifact_dir/diagnostic_best.z3.out" \
    101
run_query_cases \
    "$artifact_dir/local_edges.smt2" \
    "$artifact_dir/local_edges.z3.out" \
    303
structured_lemma_count="$(awk -F '=' '$1 == "lemmas" { print $2 }' "$artifact_dir/structured_export_summary.txt")"
structured_bridge_count="$(awk -F '=' '$1 == "bridges" { print $2 }' "$artifact_dir/structured_export_summary.txt")"
run_query_cases \
    "$artifact_dir/structured_lemmas.smt2" \
    "$artifact_dir/structured_lemmas.z3.out" \
    "$structured_lemma_count"
run_query_cases \
    "$artifact_dir/structured_bridges.smt2" \
    "$artifact_dir/structured_bridges.z3.out" \
    "$structured_bridge_count"

parse_results() {
    local suite="$1"
    local input="$2"
    awk -v suite="$suite" '
        /^(required|diagnostic|edge[0-2]|structured)_/ { id=$0; next }
        /^(unsat|sat|unknown)$/ {
            if (id == "") {
                print "missing query id before result " $0 > "/dev/stderr"
                exit 3
            }
            print suite "\t" id "\t" toupper($0)
            id=""
        }
        /^\(error/ {
            print "Z3 error: " $0 > "/dev/stderr"
            exit 4
        }
    ' "$input"
}

unknown_ids() {
    awk '
        /^(required|diagnostic)_/ { id=$0; next }
        /^unknown$/ { print id }
    ' "$1"
}

local_unknown_ids() {
    awk '
        /^edge[0-2]_/ { id=$0; next }
        /^unknown$/ { print id }
    ' "$1"
}

structured_unknown_ids() {
    local kind="$1"
    local input="$2"
    awk -v prefix="structured_${kind}_" '
        index($0, prefix) == 1 { id=$0; next }
        /^unknown$/ { print id }
    ' "$input"
}

filter_bit_queries() {
    local ids="$1"
    local input="$2"
    local output="$3"
    awk '
        NR == FNR { wanted[$1]=1; next }
        FNR <= 3 { print; next }
        {
            if (depth == 0 && $0 == "(push)") {
                block=$0 ORS
                keep=0
                depth=1
                next
            }
            if (depth > 0) {
                block=block $0 ORS
                if ($0 == "(push)") depth++
                if ($0 == "(pop)") depth--
                if ($0 ~ /^\(echo "/) {
                    id=$0
                    sub(/^\(echo "/, "", id)
                    sub(/_bits[0-9]+_[0-9]+"\)$/, "", id)
                    if (wanted[id]) keep=1
                }
                if (depth == 0) {
                    if (keep) printf "%s", block
                    block=""
                }
            }
        }
    ' "$ids" "$input" > "$output"
}

run_bit_cases() (
    local input="$1"
    local output="$2"
    local expected="$3"
    if [[ "$reuse" == 1 && "$(result_count "$output")" == "$expected" ]]; then
        return
    fi
    local work_dir current_files
    work_dir="$output.work"
    current_files="$work_dir/current_files.txt"
    mkdir -p "$work_dir"
    printf '' > "$current_files"

    # Seed the persistent per-case cache from a previously interrupted
    # aggregate, if any. A case is reused only when all four slice results are
    # present.
    if [[ -f "$output" ]]; then
        awk -v work_dir="$work_dir" '
            /_bits[0-9]+_[0-9]+$/ {
                id=$0
                sub(/_bits[0-9]+_[0-9]+$/, "", id)
                file=work_dir "/" id ".smt2.out"
                if (!seeded[id]) {
                    printf "" > file
                    close(file)
                    seeded[id]=1
                }
                print >> file
                next
            }
            /^(unsat|sat|unknown)$/ && file != "" { print >> file }
        ' "$output"
    fi

    awk -v work_dir="$work_dir" -v current_files="$current_files" '
        FNR <= 3 { header=header $0 ORS; next }
        {
            if (depth == 0 && $0 == "(push)") {
                block=$0 ORS
                id=""
                depth=1
                next
            }
            if (depth > 0) {
                block=block $0 ORS
                if ($0 == "(push)") depth++
                if ($0 == "(pop)") depth--
                if (id == "" && $0 ~ /^\(echo "/) {
                    id=$0
                    sub(/^\(echo "/, "", id)
                    sub(/_bits[0-9]+_[0-9]+"\)$/, "", id)
                }
                if (depth == 0) {
                    file=work_dir "/" id ".smt2"
                    printf "%s%s", header, block > file
                    close(file)
                    print file >> current_files
                    block=""
                }
            }
        }
    ' "$input"
    export z3_bin
    xargs -r -P "$parallel_jobs" -I '{}' \
        bash -c '
            count="$(grep -Ec "^(unsat|sat|unknown)$" "$1.out" 2>/dev/null || true)"
            if [[ "$count" != 4 ]]; then
                "$z3_bin" -smt2 "$1" > "$1.out.tmp"
                mv "$1.out.tmp" "$1.out"
            fi
        ' _ '{}' \
        < "$current_files"
    while IFS= read -r file; do
        count="$(result_count "$file.out")"
        if [[ "$count" != 4 ]]; then
            echo "Incomplete slice cache: $file.out ($count/4)" >&2
            exit 5
        fi
    done < "$current_files"
    while IFS= read -r file; do
        cat "$file.out"
    done < <(sort "$current_files") > "$output.tmp"
    mv "$output.tmp" "$output"
)

unknown_ids \
    "$artifact_dir/required_rewrites.z3.out" \
    > "$artifact_dir/required_unknown_ids.txt"
unknown_ids \
    "$artifact_dir/diagnostic_best.z3.out" \
    > "$artifact_dir/diagnostic_unknown_ids.txt"
filter_bit_queries \
    "$artifact_dir/required_unknown_ids.txt" \
    "$artifact_dir/required_rewrite_bits.smt2" \
    "$artifact_dir/required_unknown_bits.smt2"
filter_bit_queries \
    "$artifact_dir/diagnostic_unknown_ids.txt" \
    "$artifact_dir/diagnostic_best_bits.smt2" \
    "$artifact_dir/diagnostic_unknown_bits.smt2"
local_unknown_ids \
    "$artifact_dir/local_edges.z3.out" \
    > "$artifact_dir/all_local_unknown_ids.txt"
awk -F '\t' '
    NR == FNR { required_unknown[$1]=1; next }
    FNR > 1 && required_unknown[$4] {
        print $5
        print $6
        print $7
    }
' \
    "$artifact_dir/required_unknown_ids.txt" \
    "$artifact_dir/cases.tsv" \
    | sort -u \
    > "$artifact_dir/required_case_edge_ids.txt"
awk '
    NR == FNR { local_unknown[$1]=1; next }
    local_unknown[$1] { print }
' \
    "$artifact_dir/all_local_unknown_ids.txt" \
    "$artifact_dir/required_case_edge_ids.txt" \
    > "$artifact_dir/target_local_unknown_ids.txt"
filter_bit_queries \
    "$artifact_dir/target_local_unknown_ids.txt" \
    "$artifact_dir/local_edge_bits.smt2" \
    "$artifact_dir/target_local_unknown_bits.smt2"
structured_unknown_ids lemma \
    "$artifact_dir/structured_lemmas.z3.out" \
    > "$artifact_dir/structured_lemma_unknown_ids.txt"
structured_unknown_ids bridge \
    "$artifact_dir/structured_bridges.z3.out" \
    > "$artifact_dir/structured_bridge_unknown_ids.txt"
filter_bit_queries \
    "$artifact_dir/structured_lemma_unknown_ids.txt" \
    "$artifact_dir/structured_lemma_bits.smt2" \
    "$artifact_dir/structured_lemma_unknown_bits.smt2"
filter_bit_queries \
    "$artifact_dir/structured_bridge_unknown_ids.txt" \
    "$artifact_dir/structured_bridge_bits.smt2" \
    "$artifact_dir/structured_bridge_unknown_bits.smt2"

run_bit_cases \
    "$artifact_dir/required_unknown_bits.smt2" \
    "$artifact_dir/required_rewrite_bits.z3.out" \
    "$(( $(wc -l < "$artifact_dir/required_unknown_ids.txt") * 4 ))"
run_bit_cases \
    "$artifact_dir/diagnostic_unknown_bits.smt2" \
    "$artifact_dir/diagnostic_best_bits.z3.out" \
    "$(( $(wc -l < "$artifact_dir/diagnostic_unknown_ids.txt") * 4 ))"
run_bit_cases \
    "$artifact_dir/target_local_unknown_bits.smt2" \
    "$artifact_dir/local_edge_bits.z3.out" \
    "$(( $(wc -l < "$artifact_dir/target_local_unknown_ids.txt") * 4 ))"
run_bit_cases \
    "$artifact_dir/structured_lemma_unknown_bits.smt2" \
    "$artifact_dir/structured_lemma_bits.z3.out" \
    "$(( $(wc -l < "$artifact_dir/structured_lemma_unknown_ids.txt") * 4 ))"
run_bit_cases \
    "$artifact_dir/structured_bridge_unknown_bits.smt2" \
    "$artifact_dir/structured_bridge_bits.z3.out" \
    "$(( $(wc -l < "$artifact_dir/structured_bridge_unknown_ids.txt") * 4 ))"

{
    printf 'suite\tid\tresult\n'
    parse_results required "$artifact_dir/required_rewrites.z3.out"
    parse_results diagnostic "$artifact_dir/diagnostic_best.z3.out"
    parse_results local "$artifact_dir/local_edges.z3.out"
    parse_results structured_lemma "$artifact_dir/structured_lemmas.z3.out"
    parse_results structured_bridge "$artifact_dir/structured_bridges.z3.out"
} > "$artifact_dir/direct_results.tsv"

{
    printf 'suite\tid\tresult\n'
    parse_results required_bits "$artifact_dir/required_rewrite_bits.z3.out"
    parse_results diagnostic_bits "$artifact_dir/diagnostic_best_bits.z3.out"
    parse_results local_bits "$artifact_dir/local_edge_bits.z3.out"
    parse_results structured_lemma_bits "$artifact_dir/structured_lemma_bits.z3.out"
    parse_results structured_bridge_bits "$artifact_dir/structured_bridge_bits.z3.out"
} > "$artifact_dir/bit_results.tsv"

effective_results() {
    local suite="$1"
    local output="$2"
    awk -F '\t' -v suite="$suite" '
        BEGIN { OFS="\t" }
        NR == FNR {
            if (FNR == 1) next
            base=$2
            sub(/_bits[0-9]+_[0-9]+$/, "", base)
            bit_count[base]++
            if ($3 == "SAT") bit_sat[base]++
            if ($3 == "UNKNOWN") bit_unknown[base]++
            next
        }
        FNR == 1 {
            print "suite", "id", "result", "method"
            next
        }
        $1 == suite {
            if ($3 == "UNSAT" || $3 == "SAT") {
                print $1, $2, $3, "direct"
            } else if (bit_sat[$2] > 0) {
                print $1, $2, "SAT", "bit-fallback"
            } else if (bit_count[$2] == 4 && bit_unknown[$2] == 0) {
                print $1, $2, "UNSAT", "bit-fallback"
            } else {
                print $1, $2, "UNKNOWN", "bit-fallback"
            }
        }
    ' "$artifact_dir/bit_results.tsv" "$artifact_dir/direct_results.tsv" > "$output"
}

effective_results required "$artifact_dir/required_results.tsv"
effective_results diagnostic "$artifact_dir/diagnostic_results.tsv"
effective_results local "$artifact_dir/local_results.tsv"
effective_results structured_lemma "$artifact_dir/structured_lemma_results.tsv"
effective_results structured_bridge "$artifact_dir/structured_bridge_results.tsv"

awk -F '\t' '
    BEGIN { OFS="\t" }
    FILENAME == ARGV[1] { if (FNR > 1) lemma[$2]=$3; next }
    FILENAME == ARGV[2] { if (FNR > 1) bridge[$2]=$3; next }
    FILENAME == ARGV[3] && FNR == 1 {
        print "dataset", "line", "result", "bridge_result", "lemmas", "unsat", "sat", "unknown"
        next
    }
    FILENAME == ARGV[3] {
        count=split($5, ids, ",")
        if ($5 == "") count=0
        unsat=0; sat=0; unknown=0
        for (position=1; position<=count; position++) {
            if (lemma[ids[position]] == "UNSAT") unsat++
            else if (lemma[ids[position]] == "SAT") sat++
            else unknown++
        }
        bridge_result=bridge[$3]
        status=(sat > 0 || bridge_result == "SAT") ? "SAT" : ((unsat == count && bridge_result == "UNSAT") ? "UNSAT" : "UNKNOWN")
        print $1, $2, status, bridge_result, count, unsat, sat, unknown
    }
' \
    "$artifact_dir/structured_lemma_results.tsv" \
    "$artifact_dir/structured_bridge_results.tsv" \
    "$artifact_dir/structured_cases.tsv" \
    > "$artifact_dir/structured_case_results.tsv"

{
    printf 'kind\thash\toccurrences\tlines\n'
    awk -F '\t' '
    BEGIN { OFS="\t" }
    FILENAME == ARGV[1] && FNR > 1 { required_result[$2]=$3; required_method[$2]=$4; next }
    FILENAME == ARGV[2] && FNR > 1 { local_result[$2]=$3; next }
    FILENAME == ARGV[3] && FNR > 1 { structured_result[$1 SUBSEP $2]=$3; next }
    FILENAME == ARGV[4] && FNR == 1 {
        print "dataset", "line", "retained_pass", "required_id", "required_result", "chain_result", "structured_result", "gate_result", "method", "required_pair_hash", "required_residual_hash", "chain_hash", "edge0_result", "edge1_result", "edge2_result"
        next
    }
    FILENAME == ARGV[4] && FNR > 1 {
        required=required_result[$4]
        edge0=local_result[$5]
        edge1=local_result[$6]
        edge2=local_result[$7]
        structured=structured_result[$1 SUBSEP $2]
        if (structured == "") structured="N/A"
        chain=(edge0 == "UNSAT" && edge1 == "UNSAT" && edge2 == "UNSAT") ? "UNSAT" : ((edge0 == "SAT" || edge1 == "SAT" || edge2 == "SAT") ? "SAT" : "UNKNOWN")
        if (required == "SAT" || chain == "SAT" || structured == "SAT") {
            gate="SAT"; method="counterexample"
        } else if (required == "UNSAT") {
            gate="VALIDATED"; method=required_method[$4]
        } else if (chain == "UNSAT") {
            gate="VALIDATED"; method="local-chain"
        } else if (structured == "UNSAT") {
            gate="VALIDATED"; method="structured-chain"
        } else {
            gate="UNKNOWN"; method="incomplete"
        }
        print $1, $2, $3, $4, required, chain, structured, gate, method, $14, $15, $16, edge0, edge1, edge2
    }
' \
    "$artifact_dir/required_results.tsv" \
    "$artifact_dir/local_results.tsv" \
    "$artifact_dir/structured_case_results.tsv" \
    "$artifact_dir/cases.tsv" \
    > "$artifact_dir/gate_cases.tsv"

awk -F '\t' '
    BEGIN { OFS="\t" }
    FILENAME == ARGV[1] { if (FNR > 1) required_status[$2]=$3; next }
    FILENAME == ARGV[2] { if (FNR > 1) diagnostic_status[$2]=$3; next }
    FILENAME == ARGV[3] && FNR == 1 {
        print "dataset", "line", "retained_pass", "retained_hash", "retained_ast", "retained_z3", "best_pass", "best_hash", "best_ast", "best_z3"
        next
    }
    FILENAME == ARGV[3] {
        print $1, $2, $3, $15, $22, required_status[$5], $4, $16, $23, diagnostic_status[$6]
    }
' \
    "$artifact_dir/required_results.tsv" \
    "$artifact_dir/diagnostic_results.tsv" \
    "$artifact_dir/rewrite_comparison.tsv" \
    > "$artifact_dir/comparison_status.tsv"

awk -F '\t' '
    BEGIN { OFS="\t" }
    NR == FNR { if (FNR > 1) result[$2]=$3; next }
    FNR > 1 && result[$4] == "UNKNOWN" {
        pair_lines[$14]=pair_lines[$14] (pair_lines[$14] ? "," : "") $1 ":" $2
        pair_count[$14]++
        residual_lines[$15]=residual_lines[$15] (residual_lines[$15] ? "," : "") $1 ":" $2
        residual_count[$15]++
        chain_lines[$16]=chain_lines[$16] (chain_lines[$16] ? "," : "") $1 ":" $2
        chain_count[$16]++
    }
    END {
        for (hash in pair_count) print "pair", hash, pair_count[hash], pair_lines[hash]
        for (hash in residual_count) print "residual", hash, residual_count[hash], residual_lines[hash]
        for (hash in chain_count) print "chain", hash, chain_count[hash], chain_lines[hash]
    }
    ' \
        "$artifact_dir/required_results.tsv" \
        "$artifact_dir/cases.tsv" \
        | sort -t $'\t' -k1,1 -k2,2
} > "$artifact_dir/required_unknown_dedup.tsv"

{
    printf 'kind\thash\toccurrences\tedges\n'
    awk -F '\t' '
    BEGIN { OFS="\t" }
    NR == FNR { if (FNR > 1) result[$2]=$3; next }
    FNR > 1 && result[$4] == "UNKNOWN" {
        pair_lines[$9]=pair_lines[$9] (pair_lines[$9] ? "," : "") $1 ":" $2 ":" $3
        pair_count[$9]++
        residual_lines[$10]=residual_lines[$10] (residual_lines[$10] ? "," : "") $1 ":" $2 ":" $3
        residual_count[$10]++
    }
    END {
        for (hash in pair_count) print "pair", hash, pair_count[hash], pair_lines[hash]
        for (hash in residual_count) print "residual", hash, residual_count[hash], residual_lines[hash]
    }
    ' \
        "$artifact_dir/local_results.tsv" \
        "$artifact_dir/local_edges.tsv" \
        | sort -t $'\t' -k1,1 -k2,2
} > "$artifact_dir/local_unknown_dedup.tsv"

required_total="$(awk 'NR > 1 { count++ } END { print count + 0 }' "$artifact_dir/gate_cases.tsv")"
validated="$(awk -F '\t' 'NR > 1 && $8 == "VALIDATED" { count++ } END { print count + 0 }' "$artifact_dir/gate_cases.tsv")"
sat="$(awk -F '\t' 'NR > 1 && $8 == "SAT" { count++ } END { print count + 0 }' "$artifact_dir/gate_cases.tsv")"
incomplete="$(awk -F '\t' 'NR > 1 && $8 == "UNKNOWN" { count++ } END { print count + 0 }' "$artifact_dir/gate_cases.tsv")"
direct_unsat="$(awk -F '\t' 'NR > 1 && $5 == "UNSAT" { count++ } END { print count + 0 }' "$artifact_dir/gate_cases.tsv")"
chain_unsat="$(awk -F '\t' 'NR > 1 && $6 == "UNSAT" { count++ } END { print count + 0 }' "$artifact_dir/gate_cases.tsv")"
structured_unsat="$(awk -F '\t' 'NR > 1 && $7 == "UNSAT" { count++ } END { print count + 0 }' "$artifact_dir/gate_cases.tsv")"
diagnostic_unsat="$(awk -F '\t' 'NR > 1 && $3 == "UNSAT" { count++ } END { print count + 0 }' "$artifact_dir/diagnostic_results.tsv")"
diagnostic_sat="$(awk -F '\t' 'NR > 1 && $3 == "SAT" { count++ } END { print count + 0 }' "$artifact_dir/diagnostic_results.tsv")"
diagnostic_unknown="$(awk -F '\t' 'NR > 1 && $3 == "UNKNOWN" { count++ } END { print count + 0 }' "$artifact_dir/diagnostic_results.tsv")"
required_unknown_pairs="$(awk -F '\t' '$1 == "pair" { count++ } END { print count + 0 }' "$artifact_dir/required_unknown_dedup.tsv")"
required_unknown_residuals="$(awk -F '\t' '$1 == "residual" { count++ } END { print count + 0 }' "$artifact_dir/required_unknown_dedup.tsv")"

{
    printf 'roadmap=M2\n'
    printf 'required_total=%s\n' "$required_total"
    printf 'validated=%s\n' "$validated"
    printf 'direct_unsat=%s\n' "$direct_unsat"
    printf 'complete_local_chains=%s\n' "$chain_unsat"
    printf 'complete_structured_chains=%s\n' "$structured_unsat"
    printf 'sat=%s\n' "$sat"
    printf 'incomplete_chains=%s\n' "$incomplete"
    printf 'required_unknown_unique_pairs=%s\n' "$required_unknown_pairs"
    printf 'required_unknown_unique_residuals=%s\n' "$required_unknown_residuals"
    if [[ "$required_total" == 101 && "$validated" == 101 && "$sat" == 0 && "$incomplete" == 0 ]]; then
        printf 'gate=PASS\n'
    else
        printf 'gate=FAIL\n'
    fi
} > "$artifact_dir/m2_gate_summary.txt"

{
    printf 'diagnostic_total=101\n'
    printf 'diagnostic_unsat=%s\n' "$diagnostic_unsat"
    printf 'diagnostic_sat=%s\n' "$diagnostic_sat"
    printf 'diagnostic_unknown=%s\n' "$diagnostic_unknown"
    printf 'blocks_m2_gate=false\n'
} > "$artifact_dir/diagnostic_summary.txt"

cat "$artifact_dir/m2_gate_summary.txt"
cat "$artifact_dir/diagnostic_summary.txt"
grep -q '^gate=PASS$' "$artifact_dir/m2_gate_summary.txt"
