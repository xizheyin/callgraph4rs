#!/usr/bin/env bash
set -euo pipefail

# Usage: ./fetch_crate.sh <crate_name> <version>
# Example: ./fetch_crate.sh serde 1.0.203

if [[ $# -ne 2 ]]; then
  echo "Usage: $0 <crate_name> <version>" >&2
  exit 1
fi

CRATE_NAME="$1"
CRATE_VER="$2"
DEST_DIR="$(pwd)"
TARGET_PATH="${DEST_DIR}/${CRATE_NAME}-${CRATE_VER}"
URL="https://crates.io/api/v1/crates/${CRATE_NAME}/${CRATE_VER}/download"

mkdir -p "${DEST_DIR}"

if [[ -d "${TARGET_PATH}" ]]; then
  echo "Already exists: ${TARGET_PATH} (skipping download)"
  exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT

ARCHIVE_PATH="${TMPDIR}/${CRATE_NAME}-${CRATE_VER}.crate"
echo "Downloading: ${URL}"
curl -fL -o "${ARCHIVE_PATH}" "${URL}"

echo "Extracting to: ${DEST_DIR}"
tar -xzf "${ARCHIVE_PATH}" -C "${DEST_DIR}"

if [[ ! -d "${TARGET_PATH}" ]]; then
  echo "Expected directory not found after extraction: ${TARGET_PATH}" >&2
  exit 2
fi

echo "Done: ${TARGET_PATH}"
exit 0

