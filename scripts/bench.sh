#!/usr/bin/env bash
# Measures Cartograph on a large corpus. Run it inside the dev container:
#
#   CARGO_TARGET_DIR=/work/target-bench ./dev.sh cargo build --release -p cli
#   DEV_MOUNTS="-v /home/claude/bench-corpus:/corpus" CARGO_TARGET_DIR=/work/target-bench \
#     ./dev.sh bash scripts/bench.sh /corpus/linux
#
# It builds the graph in <corpus>/.cartograph (removed first), edits corpus files to test
# incremental runs and puts them back afterwards. Peak memory is the VmHWM of the process,
# polled every 100 ms, because the container has no /usr/bin/time.
#
# Knobs (environment; dev.sh only forwards what DEV_MOUNTS names, so pass them as "-e NAME=value"
# inside DEV_MOUNTS, for example DEV_MOUNTS="... -e STAGES=index"): CARTOGRAPH_BIN, RUNS (queries per latency series, default 50),
# CONCURRENT (parallel queries, default 30), QUERY_SYMBOL, MAP_PORT, SCRATCH, STAGES.
set -euo pipefail

corpus=${1:?usage: bench.sh CORPUS_DIR}
bin=${CARTOGRAPH_BIN:-${CARGO_TARGET_DIR:-/work/target}/release/cartograph}
runs=${RUNS:-50}
concurrent=${CONCURRENT:-30}
symbol=${QUERY_SYMBOL:-kmalloc}
map_port=${MAP_PORT:-18787}
scratch=${SCRATCH:-/dev/shm/cartograph-bench}
stages=${STAGES:-index incremental latency concurrent cluster map}
poll_interval=0.1
backups=$scratch/backups
cartograph=("$bin" --root "$corpus")

mkdir -p "$scratch" "$backups"
trap restore_files EXIT

now_ns() { date +%s%N; }

report() { printf '%-34s %s\n' "$1" "$2"; }

# Runs a command and prints "<wall seconds> <peak MiB>" for it; its output goes to $scratch/last.out.
measure() {
  local start end pid peak_kb=0 kb status=0
  start=$(now_ns)
  "$@" >"$scratch/last.out" 2>&1 &
  pid=$!
  while kill -0 "$pid" 2>/dev/null; do
    kb=$(awk '/^VmHWM/ {print $2}' "/proc/$pid/status" 2>/dev/null || true)
    [ -n "$kb" ] && peak_kb=$kb
    sleep "$poll_interval"
  done
  wait "$pid" || status=$?
  end=$(now_ns)
  [ "$status" -eq 0 ] || { echo "command failed ($status): $*" >&2; cat "$scratch/last.out" >&2; exit 1; }
  awk -v ns=$((end - start)) -v kb="$peak_kb" 'BEGIN {printf "%.2f %.0f\n", ns / 1e9, kb / 1024}'
}

# Milliseconds a command takes, output discarded.
milliseconds() {
  local start
  start=$(now_ns)
  "$@" >/dev/null 2>&1 || true
  echo $(( ($(now_ns) - start) / 1000000 ))
}

# "p50 p95 max" in milliseconds for $runs runs of a command.
latency() {
  local run
  for ((run = 0; run < runs; run++)); do milliseconds "$@"; done |
    sort -n |
    awk '{v[NR] = $1} END {printf "p50=%dms p95=%dms max=%dms\n", v[int((NR + 1) / 2)], v[int(NR * 0.95 + 0.5)], v[NR]}'
}

graph_size() {
  du -sk --apparent-size "$corpus/.cartograph"/* 2>/dev/null |
    awk '{printf "%s=%.0fMiB ", $2, $1 / 1024}' | sed "s#$corpus/.cartograph/##g"
  echo
}

source_files() { # n files of the corpus that have a parsed language, spread over the tree
  find "$corpus" -path "$corpus/.git" -prune -o -name '*.c' -type f -print | sort | awk -v n="$1" '{f[NR] = $0} END {step = int(NR / n); for (i = 0; i < n; i++) print f[i * step + 1]}'
}

edit_files() { # appends a line to each file named on stdin, keeping a copy to restore
  local file
  while IFS= read -r file; do
    if [ ! -e "$backups/$file" ]; then
      mkdir -p "$backups/$(dirname "$file")"
      cp -p "$file" "$backups/$file"
    fi
    printf '\n/* bench edit */\n' >>"$file"
  done
}

restore_files() { # copies every backed-up file over its edited original
  [ -d "$backups" ] || return 0
  (cd "$backups" && find . -type f | sed 's#^\./##') | while IFS= read -r file; do
    cp "$backups/$file" "/$file"
  done
  rm -rf "$backups"
}

stage_index() {
  rm -rf "$corpus/.cartograph"
  read -r seconds peak < <(measure "${cartograph[@]}" index --json)
  report "full index wall" "${seconds}s"
  report "full index peak RSS" "${peak} MiB"
  report "index report" "$(cat "$scratch/last.out")"
  report "graph dir sizes" "$(graph_size)"
  "${cartograph[@]}" coverage --budget 400 >"$scratch/coverage.out"
  report "coverage" "$(head -12 "$scratch/coverage.out" | tr '\n' '|')"
}

stage_incremental() {
  local count seconds peak
  for count in 1 100; do
    source_files "$count" | edit_files
    read -r seconds peak < <(measure "${cartograph[@]}" index --json)
    report "incremental index, $count edited" "${seconds}s peak ${peak} MiB"
    report "  report" "$(cat "$scratch/last.out")"
  done
  read -r seconds peak < <(measure "${cartograph[@]}" index --json)
  report "re-index, nothing changed" "${seconds}s peak ${peak} MiB"
  report "  report" "$(cat "$scratch/last.out")"
}

stage_latency() {
  report "query: find" "$(latency "${cartograph[@]}" find "$symbol")"
  report "query: callers" "$(latency "${cartograph[@]}" callers "$symbol")"
  report "query: impact" "$(latency "${cartograph[@]}" impact "$symbol")"
  report "query: overview" "$(latency "${cartograph[@]}" overview)"
}

stage_concurrent() {
  local run start
  source_files 50 | edit_files
  start=$(now_ns)
  for ((run = 0; run < concurrent; run++)); do
    "${cartograph[@]}" find "$symbol" >"$scratch/concurrent-$run.out" 2>&1 &
  done
  wait
  report "$concurrent concurrent queries, 50 edited" "wall $(( ($(now_ns) - start) / 1000000 ))ms"
  report "  re-indexed note" "$(grep -l 're-indexed' "$scratch"/concurrent-*.out | wc -l) processes"
  report "  stale warning" "$(grep -l '^warning: .* files changed' "$scratch"/concurrent-*.out | wc -l) processes"
  report "  indexing-in-progress note" "$(grep -l 'indexing in progress' "$scratch"/concurrent-*.out | wc -l) processes"
  report "  errors" "$(grep -l '^error' "$scratch"/concurrent-*.out | wc -l) processes"
  rm -f "$scratch"/concurrent-*.out
}

stage_cluster() {
  read -r seconds peak < <(measure "${cartograph[@]}" cluster)
  report "cluster wall" "${seconds}s"
  report "cluster peak RSS" "${peak} MiB"
  report "  output" "$(cat "$scratch/last.out")"
}

http_get_ms() {
  local start
  start=$(now_ns)
  exec 3<>"/dev/tcp/127.0.0.1/$map_port"
  printf 'GET %s HTTP/1.0\r\n\r\n' "$1" >&3
  cat <&3 >/dev/null
  exec 3<&- 3>&-
  echo $(( ($(now_ns) - start) / 1000000 ))
}

stage_map() {
  "$bin" --root "$corpus" map --bind "127.0.0.1:$map_port" >"$scratch/map.out" 2>&1 &
  local pid=$!
  trap 'kill $pid 2>/dev/null || true' RETURN
  sleep 2
  local run
  report "map /api/level (top)" "$(for ((run = 0; run < runs; run++)); do http_get_ms /api/level; done |
    sort -n | awk '{v[NR] = $1} END {printf "p50=%dms p95=%dms max=%dms\n", v[int((NR + 1) / 2)], v[int(NR * 0.95 + 0.5)], v[NR]}')"
  report "map server peak RSS" "$(awk '/^VmHWM/ {printf "%.0f MiB", $2 / 1024}' "/proc/$pid/status")"
}

for stage in $stages; do "stage_$stage"; done
