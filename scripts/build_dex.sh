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
# androidx 依赖从本机 gradle 缓存取，换机器用 ANDROIDX_CACHE 指路。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SDK="${ANDROID_HOME:-/opt/android-sdk}"
BUILD_TOOLS="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)"
PLATFORM="$SDK/platforms/android-34/android.jar"
CACHE="${ANDROIDX_CACHE:-$HOME/.gradle/caches/modules-2/files-2.1}"
OUT="$ROOT/dist"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

# GameActivity 上游继承 AppCompatActivity，本仓缓存里没有 appcompat；
# vendor 里那份只把它改成继承 androidx.activity.ComponentActivity（文件里没有 AppCompat 专有调用）。
NEED=(
  androidx.activity/activity
  androidx.core/core
  androidx.core/core-viewtree
  androidx.annotation/annotation-jvm
  androidx.annotation/annotation-experimental
  androidx.collection/collection-jvm
  androidx.lifecycle/lifecycle-common
  androidx.lifecycle/lifecycle-runtime
  androidx.lifecycle/lifecycle-viewmodel
  androidx.lifecycle/lifecycle-viewmodel-savedstate
  androidx.lifecycle/lifecycle-livedata-core
  androidx.arch.core/core-common
  androidx.arch.core/core-runtime
  androidx.savedstate/savedstate
  androidx.tracing/tracing
  androidx.versionedparcelable/versionedparcelable
  androidx.interpolator/interpolator
  androidx.navigationevent/navigationevent-android
  org.jetbrains.kotlin/kotlin-stdlib
)

mkdir -p "$STAGE/jars" "$STAGE/aars" "$STAGE/gen"
for ga in "${NEED[@]}"; do
  g="${ga%/*}" a="${ga#*/}"
  f="$(find "$CACHE/$g/$a" -name '*.aar' -o -name '*.jar' 2>/dev/null \
        | grep -v -- '-sources\|-javadoc' | sort -V | tail -1)"
  [ -n "$f" ] || { echo "缺 androidx 依赖: $ga（在 $CACHE 里没找到）" >&2; exit 1; }
  case "$f" in
    *.aar)
      cp "$f" "$STAGE/aars/$a.aar"
      unzip -p "$f" classes.jar > "$STAGE/jars/$a.jar"
      ;;
    *.jar) cp "$f" "$STAGE/jars/$a.jar" ;;
  esac
done

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
