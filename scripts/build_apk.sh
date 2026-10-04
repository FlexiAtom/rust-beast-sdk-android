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
# 组包链：cargo 交叉编译 cdylib → aapt2 link → 加 lib/<abi> → zipalign → apksigner
# 每一步都在 2026-10-02 的探针（/tmp/rust_apk_probe*）里实测过；本脚本是其固化。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SDK="${ANDROID_HOME:-/opt/android-sdk}"
BUILD_TOOLS="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)"
PLATFORM="$SDK/platforms/android-34/android.jar"
ABIS=("${@:-aarch64}")
OUT="$ROOT/dist"
# 版本号单一来源：改这里，manifest 与产物名一起变
VERSION="0.1.3"
VERSION_CODE="4"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

# 发布签名键：刻意**不在仓库里、也不在 dist/**（dist 会被清，清了键就没了，老包再也覆盖不了）。
# 换键 = 已装机的用户必须卸载重装，所以指纹钉在下面，每次组包实测比对，不匹配就停。
KS="${BEAST_KEYSTORE:-$HOME/.beast-android/release.keystore}"
KS_PASS_FILE="${BEAST_KEYSTORE_PASS_FILE:-$KS.pass}"
KS_ALIAS="beast"
# apksigner 对两个 `file:` 口令走同一个 reader，路径相同就会把第二行读到 EOF
# （报 "end of file reached"），所以库口令与键口令必须是两个文件。
KEY_PASS_FILE="${BEAST_KEY_PASS_FILE:-$KS.$KS_ALIAS.pass}"
KS_CERT_SHA256="d211cfa81c6ef49cd1ab282f140686007bcfd50cde61285c29030e9cfb87b81c"
# 缺键在**开跑前**就报，别等 cargo 编完几分钟才死在最后一步
if [ ! -f "$KS" ] || [ ! -f "$KS_PASS_FILE" ] || [ ! -f "$KEY_PASS_FILE" ]; then
  echo "找不到发布签名键：$KS（库口令 $KS_PASS_FILE，键口令 $KEY_PASS_FILE）。" >&2
  echo "键不存在时不自动生成——换签名键会让已装机用户无法覆盖升级。" >&2
  echo "要么把键放回来，要么用 BEAST_KEYSTORE / BEAST_KEYSTORE_PASS_FILE / BEAST_KEY_PASS_FILE 指路，" >&2
  echo "要么明确换键并同步改本脚本里钉住的 KS_CERT_SHA256。" >&2
  exit 1
fi

declare -A TRIPLE=( [aarch64]=aarch64-linux-android [armv7]=armv7-linux-androideabi [x86_64]=x86_64-linux-android )
declare -A ANDROID_ABI=( [aarch64]=arm64-v8a [armv7]=armeabi-v7a [x86_64]=x86_64 )

mkdir -p "$OUT"
cat > "$STAGE/AndroidManifest.xml" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="com.flexiatom.beast"
    android:versionCode="${VERSION_CODE}" android:versionName="${VERSION}">
    <uses-sdk android:minSdkVersion="21" android:targetSdkVersion="34"/>
    <application android:label="兽音译者">
        <activity android:name="com.google.androidgamesdk.GameActivity"
                  android:label="兽音译者"
                  android:theme="@android:style/Theme.DeviceDefault.NoActionBar"
                  android:exported="true"
                  android:configChanges="keyboardHidden|orientation|screenSize|screenLayout|uiMode|density|navigation">
            <meta-data android:name="android.app.lib_name" android:value="beast_app"/>
            <intent-filter>
                <action android:name="android.intent.action.MAIN"/>
                <category android:name="android.intent.category.LAUNCHER"/>
            </intent-filter>
        </activity>
    </application>
</manifest>
EOF

cd "$ROOT"
for abi in "${ABIS[@]}"; do
  t="${TRIPLE[$abi]:-}"
  [ -n "$t" ] || { echo "未知 ABI: $abi（可选 aarch64|armv7|x86_64）" >&2; exit 1; }
  cargo build --release -p beast-app --target "$t"
  d="$ROOT/target/$t/release/libbeast_app.so"
  [ -f "$d" ] || { echo "缺产物 $d" >&2; exit 1; }
  mkdir -p "$STAGE/lib/${ANDROID_ABI[$abi]}"
  cp "$d" "$STAGE/lib/${ANDROID_ABI[$abi]}/libbeast_app.so"
done

"$ROOT/scripts/build_dex.sh"

# 组一只 APK：$1 = 产物后缀，其余 = 要塞进去的 ABI 目录名。
# manifest 只描述包（versionCode / label / activity），不含 native-code 声明，所以同一份
# manifest 可以给分 ABI 包和通用包共用；系统按包里实际存在的 lib/<abi>/ 认 ABI。
package_apk() {
  local tag="$1"; shift
  local work="$STAGE/pkg-$tag"
  mkdir -p "$work"
  "$BUILD_TOOLS/aapt2" link -o "$work/unaligned.apk" -I "$PLATFORM" \
    --manifest "$STAGE/AndroidManifest.xml"
  for a in "$@"; do
    mkdir -p "$work/lib/$a"
    cp "$STAGE/lib/$a/libbeast_app.so" "$work/lib/$a/libbeast_app.so"
  done
  cp "$OUT/classes.dex" "$work/classes.dex"
  (cd "$work" && zip -q -r unaligned.apk lib && zip -q -X unaligned.apk classes.dex)
  "$BUILD_TOOLS/zipalign" -f -p 4 "$work/unaligned.apk" "$work/aligned.apk"
  local apk="$OUT/beast-${VERSION}-${tag}.apk"
  cp "$work/aligned.apk" "$apk"
  "$BUILD_TOOLS/apksigner" sign --ks "$KS" --ks-key-alias "$KS_ALIAS" \
    --ks-pass "file:$KS_PASS_FILE" --key-pass "file:$KEY_PASS_FILE" "$apk"
  "$BUILD_TOOLS/apksigner" verify "$apk"
  # 再核一次产物里实际落下的证书指纹：防止指错了另一把"也存在"的键
  local got
  got="$("$BUILD_TOOLS/apksigner" verify --print-certs "$apk" \
        | sed -n 's/.*certificate SHA-256 digest: //p' | head -1 | tr 'A-Z' 'a-z')"
  if [ "$got" != "$KS_CERT_SHA256" ]; then
    echo "$tag: 签名指纹与钉住的发布键不符：产物 $got ≠ 预期 $KS_CERT_SHA256" >&2
    exit 1
  fi
  echo "$tag: 签名指纹核对通过 $got"
  echo "OK: $apk  [$(aapt_dump_abis "$apk")]"
}

aapt_dump_abis() {
  "$BUILD_TOOLS/aapt2" dump badging "$1" 2>/dev/null \
    | sed -n "s/^native-code: //p" | tr -d "'"
}

# 每只选定的 ABI 各出一个分包，另出一个把所有 ABI 都装进去的通用包
declare -A SEEN_ABI=()
for abi in "${ABIS[@]}"; do
  a="${ANDROID_ABI[$abi]}"
  if [ -n "${SEEN_ABI[$a]:-}" ]; then continue; fi
  SEEN_ABI[$a]=1
  package_apk "$a" "$a"
done
if [ "${#SEEN_ABI[@]}" -gt 1 ]; then
  package_apk "universal" "${!SEEN_ABI[@]}"
fi

ls -lh "$OUT"/beast-"${VERSION}"-*.apk
