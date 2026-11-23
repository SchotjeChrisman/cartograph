#!/usr/bin/env bash
# Runs a command inside the rust container so the host needs no toolchain.
# Cargo home and target dir live in the project (both gitignored).
set -euo pipefail
cd "$(dirname "$0")"
# DEV_MOUNTS holds extra podman options, for example "-v /data/corpus:/corpus", split on spaces.
# shellcheck disable=SC2086
exec podman run --rm --userns=keep-id -v "$PWD":/work -w /work ${DEV_MOUNTS:-} \
  -e CARGO_HOME=/work/.cargo-home -e HOME=/work/.cargo-home \
  -e CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/work/target}" \
  docker.io/library/rust:slim "$@"
