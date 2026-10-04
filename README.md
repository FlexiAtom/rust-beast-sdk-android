# 兽音译者 · 纯 Rust 安卓版

把中文和**兽音译者编码串**互相翻译的安卓 app。编码串指经过兽音译者加工出来的那串字符：
它不是一门语言，所以本仓只按它的来历称呼它，不另立名词。整个 app 从界面到编解码都是 Rust，
没有 Kotlin/Java 业务代码，也没有手写的 JNI 边界。

编解码内核与主流**兽音译者**逐字符兼容（默认字典下 `~呜嗷` + 正文 + `啊`），
因此本仓产出的编码串可以直接贴进主流工具，反之亦然。主流串的头尾是字典序号写出来的，
四个位置恰好覆盖整套字典一次，所以**解一条主流串不需要事先知道它的字典**——字典从串里提。

## 用法

一个文本框，底部一行四个控件：

| 操作 | 行为 |
| --- | --- |
| 编码 | 框内文本 → 编码串，结果覆盖回文本框 |
| 解码 | 框内编码串 → 文本，结果覆盖回文本框 |
| 字典 | 4 个互不重复的任意字符，默认 `嗷呜啊~`。主流格式的头尾是**字典的 1 基序号**（头 `4+2+1`、尾 `3`），换字典时头尾自动跟着换 |
| 主流兼容 | 勾上 = 完整串。**解码时字典从串自己的头尾里提取**（头 `4+2+1`、尾 `3` 四个位置正好覆盖整套字典一次），提出来的字典回填进字典框，字典框填什么都不影响这一条路；编码时才用字典框那套。去掉 = 只处理裸正文，两个方向都用字典框 |
| 长按文本框 | 弹出 复制 / 粘贴 / 全选，走的是 egui 自带的长按→右键→`context_menu`。有选区就复制选区，**没选区复制整个文本框**；读写都接系统剪贴板（`ClipboardManager`），所以能和别的 app 互粘。安卓上这是必须自己接的：`egui-winit 0.31` 的 `clipboard.rs` 把 arboard 挂在 `not(target_os = "android")` 上，不接就只在 app 内部可见 |

布局：文本框吃满整块，两个按钮 + 字典 + 主流兼容挤在底部**同一行**，行下方另留出一个
`navigation_bar_height`。GameActivity 的 SurfaceView 是全屏铺底的，不留这段就会被导航栏
和屏幕圆角切掉；窄屏一行放不下时它自己折到第二行、底栏往上长高，被切的仍是留白不是控件。
顶部同理按 `status_bar_height` 留白。

解不动时（长度不是 2 的倍数、含字典外字符、该带头尾却没带）弹原生「提示」并**保留原文**，
不会把文本框清空。编码串末尾凑不满一个字的残缺字符按主流行为丢弃，但会提示丢了多少个字符。

一个格式本身带来的歧义：**勾着主流兼容时贴进裸正文**，有 253/3359 条参考实现正文（头三字符与末字符
恰好互不相同）会被读成"另一套字典的完整主流串"，解出一段自洽的错文本而不是报错——主流格式没有校验位，
这两种读法在信息上不可区分。其余会明确报错。不确定就先把主流兼容去掉。

## 构建

前置：Linux/macOS；Rust（`rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android`）、
Android SDK（platform `android-34` + 任一 build-tools）、NDK、JDK 8+、`python3`、`zip`/`unzip`/`curl`。

```bash
./scripts/build_apk.sh aarch64 armv7 x86_64   # 三个分 ABI 包 + 一个通用包
./scripts/build_apk.sh aarch64                # 只出 arm64 分包
```

产物在 `dist/beast-<版本>-<abi>.apk`，多 ABI 时另出 `dist/beast-<版本>-universal.apk`。
发版四个文件都传：**arm64-v8a**（近几年的手机基本都是它，约 11 MB）、**armeabi-v7a**（老 32 位机，
约 11 MB）、**x86_64**（模拟器 / Chromebook，约 11 MB）、**universal**（不确定机型，约 29 MB）。
分包与通用包可以互相覆盖升级：同一把签名键、同一个 `versionCode`，装哪个都行。

**别拿 `dist/beast-0.1.0-universal.apk` 发版**：它在修复当天被重出过一次，包体已含「从串里提字典」
的跨字典修复，文件名与 manifest 却仍是 `0.1.0` / `versionCode=1`，而 tag `v0.1.0` 的树并不含该修复。
以版本论事：跨字典修复随 **0.1.1** 发。

**版本号只有一个来源**：`scripts/build_apk.sh` 里的 `VERSION` / `VERSION_CODE`，改它，manifest
与全部产物名一起变。

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
相同，回拉件上的证书指纹即 `KS_CERT_SHA256`，包 flags 无 `DEBUGGABLE`。**尾部残缺提示**那条
Toast 已在真机弹出——由人在框里手敲一段末尾凑不满一个字的编码串看到（自动化喂不进去：`adb shell
input text` 对 CJK 直接 NPE），原话「已测试，提示正确弹出」。

**一处修正**：早先说的"兼容主流"只覆盖了默认字典——`decode` 是拿**本地字典**算出的头尾去匹配
输入串，换字典的主流串解不开，那条路不算真兼容（人工指出：「在勾选兼容主流时，字典应从兽语提取，
而非硬编码的字符串」）。现在勾选主流兼容走 `decode_mainstream`，字典从串的头尾提取。报错文案同样
去掉了硬编码的 `~呜嗷` / `啊`，改为按**当前字典**的头尾描述。

人工已用**真实非示例**的编码串（自定义字典 `配方复活`）装机测过，原话「人工测试通过，确认可以
兼容主流协议（主流协议指头尾附加）」⇒ 跨字典提取这条路由人工在真机确认。

**0.1.1 起出分 ABI 包**：`arm64-v8a` 分包已在真机覆盖装上 0.1.0 通用包（`versionCode` 1→2，
`Success`，启动后 `mCurrentFocus` 是本包 GameActivity），从设备回拉的 `base.apk` 与 `dist/` 那只
`sha256` 逐字节相同（`4622d69a…dbb44da`），回拉件证书指纹仍是 `KS_CERT_SHA256`。

**未证**：

- 跨字典提取缺**外部差分**：真机确认来自人工，机器侧证据是 Rust 往返（4 套字典 × 4 段文本）加上
  11 条主流实测串走同一条提取路（解出原文、提出默认字典、无残缺尾部）。手里没有一份主流工具用
  **非默认字典**产出的串可对，所以"主流工具换字典后也这么解"仍是按格式定义（头 = 1 基序号
  `4+2+1`、尾 = `3`）推的，不是差分实测出来的。
- `armeabi-v7a` / `x86_64` 两条 `.so` 只过了编译，本机没有 32 位或 x86 设备/模拟器，运行期从未执行过。
  0.1.1 的分包让这两条各自能被安装，但装机核验只做了 `arm64-v8a`。
- 整个 APK 不是字节级可复现：zip 条目带时间戳，且 `d8`（build-tools 37.0.0 / D8 9.2.4-dev）与
  `javac 17.0.20.1` 未钉版本。可复现性目前只覆盖到 `classes.dex` 这一层。
