# 兽音译者 · 纯 Rust 安卓版

把「兽语」和中文互相翻译的安卓 app。整个 app 从界面到编解码都是 Rust，没有 Kotlin/Java 业务代码，
也没有手写的 JNI 边界。

编解码内核与主流**兽音译者**逐字符兼容（默认字典下 `~呜嗷` + 正文 + `啊`），
因此本仓产出的兽语串可以直接贴进主流工具，反之亦然。

## 用法

一个文本框，两个按钮：

| 操作 | 行为 |
| --- | --- |
| 翻译为兽音 | 框内文本 → 兽语，结果覆盖回文本框 |
| 翻译为人话 | 框内兽语 → 文本，结果覆盖回文本框 |
| 字典 | 4 个互不重复的任意字符，默认 `嗷呜啊~`。主流格式的头尾是**字典的 1 基序号**（头 `4+2+1`、尾 `3`），换字典时头尾自动跟着换 |
| 主流兼容 | 勾上 = 完整串（带头尾）；去掉 = 只处理裸正文 |

解不动时（长度不是 2 的倍数、含字典外字符、该带头尾却没带）弹原生「提示」并**保留原文**，
不会把文本框清空。兽语串末尾凑不满一个字的残缺字符按主流行为丢弃，但会提示丢了多少个字符。

## 构建

前置：Linux/macOS；Rust（`rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android`）、
Android SDK（platform `android-34` + 任一 build-tools）、NDK、JDK 8+、`python3`、`zip`/`unzip`/`curl`。

```bash
./scripts/build_apk.sh aarch64 armv7 x86_64     # 通用包
./scripts/build_apk.sh aarch64                  # 只出 arm64
```

产物在 `dist/beast-<版本>-<abi 组合>.apk`。**版本号只有一个来源**：`scripts/build_apk.sh` 里的
`VERSION` / `VERSION_CODE`，改它，manifest 与产物名一起变。

组包链是 `cargo` 交叉编译 cdylib → `scripts/build_dex.sh`（`javac` + `d8`）→ `aapt2 link` →
塞 `lib/<abi>` → `zipalign` → `apksigner`，全程不走 Gradle。

> **换机器要改一处**：`.cargo/config.toml` 里的链接器与 `CC_*`/`CXX_*` 是**绝对路径**
> （`/opt/android-ndk/...`）。NDK 装在别处就改这个文件，或用同名环境变量指路。
> 这是本仓目前唯一没做成可配置的地方。

### dex 依赖

`classes.dex` 里除了本仓 `vendor/game-activity/` 的 Java，还要打进 19 个 androidx / kotlin 构件。
它们的版本与 SHA-256 钉在 **`scripts/androidx-deps.lock`**，`build_dex.sh` 只认这份锁：
先看 `dist/deps/`（已缓存则核哈希）→ 再按哈希扫本机 gradle 缓存（离线可用）→
都没有才按锁里的 URL 下载并核哈希，任何一步对不上就停下来报错，不会"挑个能编过的版本"。

锁里 19 个哈希都逐条对过发布方：16 个用 Google Maven / Maven Central 的官方 `.sha256` sidecar，
`versionedparcelable 1.1.1`、`interpolator 1.0.0`、`kotlin-stdlib 2.1.20` 三个发布方只出 `.sha1`，
改用 `.sha1` 核对。清空 `dist/deps` 并把 `ANDROIDX_CACHE` 指向空目录后重编，
`classes.dex` 与之前**逐字节相同**（5,240,828 B / `sha256 1ec82606…88e7282`）。

### 签名

签名键**不在仓库里，也不在 `dist/`**（`dist/` 会被清，键没了就会静默换签名，老包再也覆盖不了）。
默认路径：

```
~/.beast-android/release.keystore              # 密钥库（alias: beast, CN=FlexiAtom）
~/.beast-android/release.keystore.pass         # 库口令
~/.beast-android/release.keystore.beast.pass   # 键口令
```

三个文件都 `chmod 600`，**请自行做机外备份**。`build_apk.sh` 找不到它们就直接失败，不自动生成。
证书指纹（SHA-256 `d211cfa81c6ef49cd1ab282f140686007bcfd50cde61285c29030e9cfb87b81c`）钉在
`build_apk.sh` 的 `KS_CERT_SHA256`，每次组包后从产物里读回实际签名指纹比对，不符即停——
所以"指错了另一把也存在的键"编不出包来。换键必须同时改这一行，那意味着从此无法覆盖升级。

> `apksigner` 对两个 `file:` 口令走同一个 reader，库口令与键口令给同一个文件会被读到 EOF，
> 所以必须是两个文件。

## 许可

本仓库的自有代码是 **AGPL-3.0-or-later**（全文见 [`LICENSE`](LICENSE)，各源文件带 SPDX 头）。
`crates/beast-core` 是兽音译者编解码算法的 Rust 实现，其算法口径以
[beast_sdk](https://github.com/SycAlright/beast_sdk) 的 `JavaScript/beast.js` 为基准做兼容性对齐
（该上游没有 Rust 实现，本 crate 是独立编写，`tests/` 里的向量与实测样本用于差分验证）。

第三方许可与随二进制的字体文本：

| 位置 | 覆盖对象 |
| --- | --- |
| [`LICENSES/apache-2.0.txt`](LICENSES/apache-2.0.txt) | androidx / kotlin / `vendor/game-activity` 的 GameActivity（Apache-2.0） |
| [`LICENSES/egui-default-fonts.txt`](LICENSES/egui-default-fonts.txt) | eframe `default_fonts` 特征打进二进制的四份字体文本 |
| [`LICENSES/wqy-zenhei.txt`](LICENSES/wqy-zenhei.txt) | 历史提交里的 `wqy-zenhei.ttc`（GPL-2.0+ 带字体嵌入例外）。当前 HEAD 已不含该字体，但克隆仓库会取得那份副本，故附其许可文本 |
| `crates/app/assets/LICENSE-SourceHanSans.txt` | 现用中文字体思源黑体 CN Regular（OFL-1.1），8.4 MB 经 `include_bytes!` 打进 `libbeast_app.so` |

## 已证 / 未证

写清楚，免得把"编译通过"当成"设备上能用"。

**已实测**：一台 arm64 真机（API 35）安装并正常运行、中文字形完整、编解码与主流工具逐字符
兼容（3359 条向量 + 11 条实测原文差分）、`classes.dex` 依赖钉版本后可复现、签名闸能拦下错键。
**发布键签名的通用包**已装机核验一次：从设备回拉的 `base.apk` 与 `dist/` 产物 `sha256` 逐字节
相同，回拉件上的证书指纹即 `KS_CERT_SHA256`，包 flags 无 `DEBUGGABLE`。

**未证**：

- `armeabi-v7a` / `x86_64` 两条 `.so` 只过了编译，本机没有 32 位或 x86 设备/模拟器，运行期从未执行过。
- 「尾部残缺提示」这条 Toast 在真机上没被亲眼看到（自动化喂不进残缺兽语串：`adb shell input text` 对
  CJK 直接 NPE）。人工确认的是"应用正常工作"，没点名这条提示。它走的是错误提示早已实机验证过的同一条
  `notice → toast` 通路，逻辑有单测覆盖。
- 整个 APK 不是字节级可复现：zip 条目带时间戳，且 `d8`（build-tools 37.0.0 / D8 9.2.4-dev）与
  `javac 17.0.20.1` 未钉版本。可复现性目前只覆盖到 `classes.dex` 这一层。
