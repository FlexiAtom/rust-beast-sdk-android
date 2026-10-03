#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Copyright (C) 2026 FlexiAtom
#
# This program is free software: you can redistribute it and/or modify it under
# the terms of the GNU Affero General Public License as published by the Free
# Software Foundation, either version 3 of the License, or (at your option) any
# later version.
#
# This program is distributed in the hope that it will be useful, but WITHOUT ANY
# WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
# PARTICULAR PURPOSE. See the GNU Affero General Public License for more details.
#
# You should have received a copy of the GNU Affero General Public License along
# with this program. If not, see <https://www.gnu.org/licenses/>.
# 把 vendor/game-activity 的 Java 编成 dist/classes.dex。
# NDK r29 不再自带 game-activity（C++ 由 android-activity 的 build.rs 静态编进 cdylib），
# 但 Java 侧的 com.google.androidgamesdk.GameActivity 必须打进 APK。
# 依赖版本与 SHA-256 一律以 scripts/androidx-deps.lock 为准：本机 gradle 缓存里命中同版本同哈希
# 就直接用，否则按锁里的 URL 下载并核对；对不上就停，绝不"先编过再说"。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SDK="${ANDROID_HOME:-/opt/android-sdk}"
BUILD_TOOLS="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)"
PLATFORM="$SDK/platforms/android-34/android.jar"
CACHE="${ANDROIDX_CACHE:-$HOME/.gradle/caches/modules-2/files-2.1}"
LOCK="$ROOT/scripts/androidx-deps.lock"
OUT="$ROOT/dist"
DEPS="$OUT/deps"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

sha256_of() { sha256sum "$1" | cut -d' ' -f1; }

# 把锁里的一行解析成本地文件路径。锁格式：<sha256><TAB><仓库 URL>，
# URL 形如 .../maven2/<group>/<artifact>/<version>/<file>（gradle 缓存的 group 是点式）。
resolve_dep() {
  local want="$1" url="$2" rel gav group artifact version fn ext dest src
  rel="${url#*maven2/}"
  fn="${rel##*/}"
  ext="${fn##*.}"
  gav="${rel%/*}"
  version="${gav##*/}"
  artifact="${gav%/*}"; artifact="${artifact##*/}"
  group="${gav%/*}"; group="${group//\//.}"
  dest="$DEPS/$fn"

  if [ -f "$dest" ]; then
    if [ "$(sha256_of "$dest")" != "$want" ]; then
      echo "$fn 已在 $dest，但哈希不对（期望 $want）；删掉它重来" >&2
      return 1
    fi
  else
    src=""
    for c in "$CACHE/$group/$artifact/$version"/*/*."$ext"; do
      [ -f "$c" ] || continue
      if [ "$(sha256_of "$c")" = "$want" ]; then src="$c"; break; fi
    done
    if [ -n "$src" ]; then
      cp "$src" "$dest"
    else
      curl -sfL --output "$dest.part" "$url" || { rm -f "$dest.part"; echo "拉不到 $url" >&2; return 1; }
      got="$(sha256_of "$dest.part")"
      if [ "$got" != "$want" ]; then
        rm -f "$dest.part"
        echo "$url 的哈希与锁不符（期望 $want，实际 $got）" >&2
        return 1
      fi
      mv "$dest.part" "$dest"
    fi
  fi
  printf '%s\n' "$dest"
}

mkdir -p "$DEPS" "$STAGE/jars" "$STAGE/aars" "$STAGE/gen"
while IFS=$'\t' read -r want url; do
  [ -n "${want:-}" ] || continue
  f="$(resolve_dep "$want" "$url")"
  rel="${url#*maven2/}"; fn="${rel##*/}"
  a="${fn%-*}"                          # <artifact>，锁里的文件名一定是 <artifact>-<version>.<ext>
  case "$f" in
    *.aar)
      cp "$f" "$STAGE/aars/$a.aar"
      unzip -p "$f" classes.jar > "$STAGE/jars/$a.jar"
      ;;
    *.jar) cp "$f" "$STAGE/jars/$a.jar" ;;
  esac
done < <(grep -v '^#' "$LOCK")

# aar 里只有 R.txt、没有 R 类，缺了它 ViewCompat 一初始化就闪退
python3 "$ROOT/scripts/gen_androidx_r.py" "$STAGE/aars" "$STAGE/gen"

CP="$(ls "$STAGE"/jars/*.jar | tr '\n' ':')$PLATFORM"
javac -nowarn -classpath "$CP" -d "$STAGE/classes" \
  $(find "$ROOT/vendor/game-activity" -name '*.java') \
  $(find "$STAGE/gen" -name '*.java')
jar cf "$STAGE/game-activity.jar" -C "$STAGE/classes" .

mkdir -p "$STAGE/dex" "$OUT"
"$BUILD_TOOLS/d8" --min-api 21 --lib "$PLATFORM" --output "$STAGE/dex" \
  "$STAGE/game-activity.jar" "$STAGE/jars"/*.jar > "$STAGE/d8.log" 2>&1
# grep 无命中时退出码非 0，直接写在 set -e 下会把整个脚本判失败，所以包进 if
if grep -qiE '^(Error|Compilation error)' "$STAGE/d8.log"; then
  cat "$STAGE/d8.log" >&2
  exit 1
fi
cp "$STAGE/dex/classes.dex" "$OUT/classes.dex"
echo "OK: $OUT/classes.dex ($(stat -c %s "$OUT/classes.dex") 字节)"
