#!/usr/bin/env bash
set -euo pipefail

# 用法: ./fetch_crate.sh <crate_name> <version>
# 例如: ./fetch_crate.sh serde 1.0.203

if [[ $# -ne 2 ]]; then
  echo "用法: $0 <crate_name> <version>" >&2
  exit 1
fi

CRATE_NAME="$1"
CRATE_VER="$2"
DEST_DIR="$(pwd)"
TARGET_PATH="${DEST_DIR}/${CRATE_NAME}-${CRATE_VER}"
URL="https://crates.io/api/v1/crates/${CRATE_NAME}/${CRATE_VER}/download"

mkdir -p "${DEST_DIR}"

if [[ -d "${TARGET_PATH}" ]]; then
  echo "已存在: ${TARGET_PATH} (跳过下载)"
  exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT

ARCHIVE_PATH="${TMPDIR}/${CRATE_NAME}-${CRATE_VER}.crate"
echo "下载: ${URL}"
curl -fL -o "${ARCHIVE_PATH}" "${URL}"

echo "解压到: ${DEST_DIR}"
tar -xzf "${ARCHIVE_PATH}" -C "${DEST_DIR}"

if [[ ! -d "${TARGET_PATH}" ]]; then
  echo "解压后未发现目录: ${TARGET_PATH}" >&2
  exit 2
fi

echo "完成: ${TARGET_PATH}"
exit 0


