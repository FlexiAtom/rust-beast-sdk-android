#!/usr/bin/env bash
# 组包链：cargo 交叉编译 cdylib → aapt2 link → 加 lib/<abi> → zipalign → apksigner
# 每一步都在 2026-10-02 的探针（/tmp/rust_apk_probe*）里实测过；本脚本是其固化。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SDK="${ANDROID_HOME:-/opt/android-sdk}"
BUILD_TOOLS="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)"
PLATFORM="$SDK/platforms/android-34/android.jar"
ABIS=("${@:-aarch64}")
OUT="$ROOT/dist"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

declare -A TRIPLE=( [aarch64]=aarch64-linux-android [armv7]=armv7-linux-androideabi [x86_64]=x86_64-linux-android )
declare -A ANDROID_ABI=( [aarch64]=arm64-v8a [armv7]=armeabi-v7a [x86_64]=x86_64 )

mkdir -p "$OUT"
cat > "$STAGE/AndroidManifest.xml" <<'EOF'
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="com.flexiatom.beast"
    android:versionCode="1" android:versionName="0.1.0">
    <uses-sdk android:minSdkVersion="21" android:targetSdkVersion="34"/>
    <application android:label="兽音译者" android:hasCode="false">
        <activity android:name="android.app.NativeActivity"
                  android:label="兽音译者"
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

"$BUILD_TOOLS/aapt2" link -o "$STAGE/unaligned.apk" -I "$PLATFORM" \
  --manifest "$STAGE/AndroidManifest.xml"
(cd "$STAGE" && zip -q -r unaligned.apk lib)
"$BUILD_TOOLS/zipalign" -f -p 4 "$STAGE/unaligned.apk" "$STAGE/aligned.apk"

KS="$OUT/debug.keystore"
if [ ! -f "$KS" ]; then
  keytool -genkeypair -keystore "$KS" -storepass android -keypass android \
    -alias androiddebugkey -dname "CN=Debug" -keyalg RSA -keysize 2048 -validity 10000 2>/dev/null
fi
cp "$STAGE/aligned.apk" "$OUT/beast-debug.apk"
"$BUILD_TOOLS/apksigner" sign --ks "$KS" --ks-pass pass:android --key-pass pass:android "$OUT/beast-debug.apk"
"$BUILD_TOOLS/apksigner" verify "$OUT/beast-debug.apk"
echo "OK: $OUT/beast-debug.apk"
