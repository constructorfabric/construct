#!/usr/bin/env bash
# Build pgvector against the PostgreSQL release the agent bundles, from pinned,
# checksum-verified sources. The agent embeds the files it writes into OUT_DIR:
#   vector.so  vector.control  vector.sql  vector.pgversion
# Needs: curl, tar, make, gcc, sha256sum. Linux x86_64 only.
#
# usage: scripts/build-pgvector.sh <out-dir>
# The pins are in this script (pgvector release and checksums) and in .cargo/config.toml
# (PostgreSQL release); a new PostgreSQL release needs its checksum added here.
set -euo pipefail

# The PostgreSQL release is the one pinned in .cargo/config.toml (POSTGRESQL_VERSION), which is
# also the one the agent embeds. Cargo passes it in; a manual run reads it from that file.
root=$(cd "$(dirname "$0")/.." && pwd)
PG_VERSION=${POSTGRESQL_VERSION:-}
if [ -z "$PG_VERSION" ]; then
  PG_VERSION=$(sed -n 's/^POSTGRESQL_VERSION *= *"\(.*\)"/\1/p' "$root/.cargo/config.toml")
fi
PG_VERSION=${PG_VERSION#=}
# sha256 of the PostgreSQL archive of that release; a new release needs its checksum added here.
case "$PG_VERSION" in
  16.15.1) PG_SHA256="0c42e7c05f0cb596b82ac07990c96fc261ec2e8106055d557c76442d712dbf74" ;;
  *) echo "no checksum for PostgreSQL '$PG_VERSION' in $0" >&2; exit 1 ;;
esac
PG_URL="https://github.com/theseus-rs/postgresql-binaries/releases/download/${PG_VERSION}/postgresql-${PG_VERSION}-x86_64-unknown-linux-gnu.tar.gz"

PGVECTOR_VERSION="0.8.7"
PGVECTOR_SHA256="cac0b10c360f05b2d521200105ba3697e773d4cd3731f5a915a7e37ebe0bea85"
PGVECTOR_URL="https://github.com/pgvector/pgvector/archive/refs/tags/v${PGVECTOR_VERSION}.tar.gz"

out=${1:?usage: build-pgvector.sh <out-dir>}
mkdir -p "$out"
out=$(cd "$out" && pwd)
work="$out/work"
rm -rf "$work"
mkdir -p "$work"

fetch() { # url sha256 file
  # A copy from an earlier run is reused when it still matches the checksum.
  if [ -f "$out/$3" ] && echo "$2  $out/$3" | sha256sum -c --quiet 2>/dev/null; then
    return
  fi
  curl -fsSL --retry 3 -o "$out/$3" "$1"
  echo "$2  $out/$3" | sha256sum -c --quiet
}

fetch "$PG_URL" "$PG_SHA256" postgresql.tar.gz
fetch "$PGVECTOR_URL" "$PGVECTOR_SHA256" pgvector.tar.gz

mkdir "$work/pg" "$work/src"
tar -xzf "$out/postgresql.tar.gz" -C "$work/pg" --strip-components=1
tar -xzf "$out/pgvector.tar.gz" -C "$work/src" --strip-components=1

# OPTFLAGS="" drops -march=native: the build must run on every x86_64 machine.
make -C "$work/src" PG_CONFIG="$work/pg/bin/pg_config" OPTFLAGS="" >"$work/make.log" 2>&1 \
  || { cat "$work/make.log" >&2; exit 1; }

cp "$work/src/vector.so" "$out/vector.so"
cp "$work/src/vector.control" "$out/vector.control"
cp "$work/src/sql/vector--${PGVECTOR_VERSION}.sql" "$out/vector.sql"
# The agent refuses to install vector.so into another PostgreSQL release than this one.
printf '%s' "$PG_VERSION" >"$out/vector.pgversion"
rm -rf "$work"
echo "built pgvector ${PGVECTOR_VERSION} for PostgreSQL ${PG_VERSION} in $out"
