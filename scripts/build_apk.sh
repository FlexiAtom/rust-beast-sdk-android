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
VERSION="0.1.0"
VERSION_CODE="1"
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

"$BUILD_TOOLS/aapt2" link -o "$STAGE/unaligned.apk" -I "$PLATFORM" \
  --manifest "$STAGE/AndroidManifest.xml"
(cd "$STAGE" && zip -q -r unaligned.apk lib)
cp "$OUT/classes.dex" "$STAGE/classes.dex"
(cd "$STAGE" && zip -q -X unaligned.apk classes.dex)
"$BUILD_TOOLS/zipalign" -f -p 4 "$STAGE/unaligned.apk" "$STAGE/aligned.apk"

# 三个 ABI 齐全就按通用包命名，否则列出实际打包的 ABI
TAG="$(for abi in "${ABIS[@]}"; do echo "${ANDROID_ABI[$abi]}"; done | sort -u | paste -sd- -)"
[ "$TAG" = "arm64-v8a-armeabi-v7a-x86_64" ] && TAG="universal"
APK="$OUT/beast-${VERSION}-${TAG}.apk"
cp "$STAGE/aligned.apk" "$APK"

"$BUILD_TOOLS/apksigner" sign --ks "$KS" --ks-key-alias "$KS_ALIAS" \
  --ks-pass "file:$KS_PASS_FILE" --key-pass "file:$KEY_PASS_FILE" "$APK"
"$BUILD_TOOLS/apksigner" verify "$APK"
# 再核一次产物里实际落下的证书指纹：防止指错了另一把"也存在"的键
got="$("$BUILD_TOOLS/apksigner" verify --print-certs "$APK" \
      | sed -n 's/.*certificate SHA-256 digest: //p' | head -1 | tr 'A-Z' 'a-z')"
if [ "$got" != "$KS_CERT_SHA256" ]; then
  echo "签名指纹与钉住的发布键不符：产物 $got ≠ 预期 $KS_CERT_SHA256" >&2
  exit 1
fi
echo "签名指纹核对通过: $got"
ls -lh "$APK"
echo "OK: $APK"
