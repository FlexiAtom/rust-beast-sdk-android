//! Syc <github.com/SycAlright>
//! Beast_SDK Rust
//!
//! [`encode`] 产出的就是**主流兽音译者**的完整串，可直接贴进主流工具；[`decode`] 读回它。
//! 主流格式 = `嗷呜啊~` 字典序号 `4 + 2 + 1` + 正文 + `3`，其中正文与
//! `JavaScript/beast.js` 的 `encode` 逐字符一致（要那段裸正文就用 [`encode_body`] / [`decode_body`]）：
//! 把每个字符拆成 UTF-16 码元，每个码元写成定长 4 位十六进制，位流第 `n` 位输出
//! `beast[(v + n % 16) % 16]` 对应的两个字。
//!
//! 正文为什么以 JS 为基准而不是 Python：实测主流编码结果 = `~呜嗷` + `beast.js` 的 `encode` + `啊`
//! （`你好`、`A`、`B`、`AB`、`甲乙丙丁一二三四`、`😀` 等 11 条样本全部逐字符命中）。
//! 仓库里的 `Python`/`Go`/`PHP` 各自是另一套位流，见 README 的兼容性说明。
//!
//! 定长 4 位带来的两个结果，正是主流工具的行为：
//! - **位流永远 4 位对齐**，段边界与码元边界重合，所以 `decode(encode(x)) == x` 对任意输入无损，
//!   辅助平面（`😀` → `d83d de00`）也还原得回来。
//! - JS 的 `decode` 按每 4 位切段，**末尾不足 4 位直接丢弃**（`while (end <= length)`），
//!   这里照搬：不报错也不补位，手写残缺串会静默少解最后一个码元。
//!
//! Rust 的 `char` 存不了孤立代理项，`String::from_utf16_lossy` 会把它们换成 U+FFFD。
//! 只有残缺/手写的兽语串能解出孤立代理项——Rust 字符串本身不可能编出它们。

/// 兽语字典：一个十六进制位 `k`（0..15）编码为 `beast[k / 4]` + `beast[k % 4]`。
/// 换成自定义字典时须保证 4 个元素互不重复。
pub const BEAST: [char; 4] = ['嗷', '呜', '啊', '~'];

/// 字典基数：一个兽语字符承载 log2(4) = 2 位，一对承载一个十六进制位
const DICT_BASE: usize = 4;

/// 主流兽音译者附在正文前的头，即字典序号 `4 + 2 + 1`（[`BEAST`] 的第 4、2、1 个）。
/// [`encode`] 自动加上，[`decode`] 要求它在。
pub const AFFIX_HEAD: &str = "~呜嗷";

/// 主流兽音译者附在正文后的尾，即字典序号 `3`（见 [`AFFIX_HEAD`]）
pub const AFFIX_TAIL: &str = "啊";

/// 一套可替换的兽语字典（4 个互不重复的字符，1 基序号 1..4）。
///
/// 主流格式的头尾**不是固定字符串，而是字典的 1 基序号**：头 = `4 + 2 + 1`、尾 = `3`。
/// 换字典后头尾必须跟着换，这正是它兼容不同兽音的机制——默认字典下
/// [`BeastDict::head`] == [`AFFIX_HEAD`]、[`BeastDict::tail`] == [`AFFIX_TAIL`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeastDict {
    chars: [char; 4],
}

/// 字典不合法：长度不是 4，或 4 个元素有重复
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictError {
    WrongLength { char_count: usize },
    DuplicateChar(char),
}

impl std::fmt::Display for DictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DictError::WrongLength { char_count } => {
                write!(f, "字典需要恰好 4 个字符，收到 {char_count} 个")
            }
            DictError::DuplicateChar(ch) => write!(f, "字典字符 {ch:?} 重复，4 个必须互不相同"),
        }
    }
}

impl std::error::Error for DictError {}

impl BeastDict {
    /// 默认字典 = 主流兽音译者的 `嗷呜啊~`
    pub const DEFAULT: Self = Self { chars: BEAST };

    pub fn new(chars: [char; 4]) -> Result<Self, DictError> {
        for (i, a) in chars.iter().enumerate() {
            if let Some(&b) = chars[i + 1..].iter().find(|&&b| b == *a) {
                return Err(DictError::DuplicateChar(b));
            }
        }
        Ok(Self { chars })
    }

    /// 从恰好 4 个字符的字符串建字典（按 `chars()` 计数，任何 Unicode 字符都可以）
    pub fn parse(s: &str) -> Result<Self, DictError> {
        let chars: Vec<char> = s.chars().collect();
        let chars: [char; 4] = chars
            .try_into()
            .map_err(|v: Vec<char>| DictError::WrongLength {
                char_count: v.len(),
            })?;
        Self::new(chars)
    }

    pub fn chars(&self) -> [char; 4] {
        self.chars
    }

    fn index(&self, ch: char) -> Option<usize> {
        self.chars.iter().position(|&b| b == ch)
    }

    /// 主流头 = 字典 1 基序号 `4 + 2 + 1`
    pub fn head(&self) -> String {
        [self.chars[3], self.chars[1], self.chars[0]]
            .into_iter()
            .collect()
    }

    /// 主流尾 = 字典 1 基序号 `3`
    pub fn tail(&self) -> char {
        self.chars[2]
    }

    pub fn encode(&self, text: &str) -> String {
        let mut out = String::with_capacity(6 + text.len() * 8);
        out.push_str(&self.head());
        out.push_str(&self.encode_body(text));
        out.push(self.tail());
        out
    }

    pub fn encode_body(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() * 8);
        for (index, unit) in text.encode_utf16().enumerate() {
            for (offset, digit) in hex4(unit).into_iter().enumerate() {
                let k = (digit + (index * 4 + offset) % 16) % 16;
                out.push(self.chars[k / DICT_BASE]);
                out.push(self.chars[k % DICT_BASE]);
            }
        }
        out
    }

    /// 完整串 → 人话；头尾必须是**本字典**算出的，缺任一即 [`DecodeError::MissingAffix`]
    pub fn decode(&self, beast_text: &str) -> Result<String, DecodeError> {
        let core = beast_text
            .strip_prefix(&self.head())
            .and_then(|body| body.strip_suffix(self.tail()))
            .ok_or(DecodeError::MissingAffix)?;
        self.decode_body(core)
    }

    pub fn decode_body(&self, beast_text: &str) -> Result<String, DecodeError> {
        let chars: Vec<char> = beast_text.chars().collect();
        if !chars.len().is_multiple_of(2) {
            return Err(DecodeError::OddLength {
                char_count: chars.len(),
            });
        }
        let mut nibbles = Vec::with_capacity(chars.len() / 2);
        for (n, pair) in chars.chunks(2).enumerate() {
            let high = self
                .index(pair[0])
                .ok_or(DecodeError::UnknownChar(pair[0]))?;
            let low = self
                .index(pair[1])
                .ok_or(DecodeError::UnknownChar(pair[1]))?;
            let k = (high * DICT_BASE + low) as isize - (n % 16) as isize;
            nibbles.push(rem(k, 16) as u16);
        }
        let mut units = Vec::with_capacity(nibbles.len() / 4);
        for chunk in nibbles.chunks(4) {
            if chunk.len() < 4 {
                break;
            }
            units.push(chunk.iter().fold(0u16, |acc, &digit| acc * 16 + digit));
        }
        Ok(String::from_utf16_lossy(&units))
    }
}

/// 人话 → 主流兽音译者的完整串：[`AFFIX_HEAD`] + 正文 + [`AFFIX_TAIL`]
pub fn encode(text: &str) -> String {
    BeastDict::DEFAULT.encode(text)
}

/// 人话 → 兽语正文（不带 [`AFFIX_HEAD`] / [`AFFIX_TAIL`]，与 `JavaScript/beast.js` 同码）
pub fn encode_body(text: &str) -> String {
    BeastDict::DEFAULT.encode_body(text)
}

/// 主流完整串 → 人话，要求 [`AFFIX_HEAD`] 与 [`AFFIX_TAIL`] **都在**，缺任一即
/// [`DecodeError::MissingAffix`]。裸正文交给 [`decode_body`]。
///
/// 这里刻意不做"看起来像就剥掉"的自动识别：正文本身可以正好以 `~呜嗷` 开头、以 `啊` 结尾，
/// 例如 `퀃`（U+D003）的正文是 `~呜嗷呜嗷啊呜啊`。按长度 4 剥掉会把整条位流挪动 2 位，
/// 后面每个码元都解错。
pub fn decode(beast_text: &str) -> Result<String, DecodeError> {
    BeastDict::DEFAULT.decode(beast_text)
}

/// 兽语正文 → 人话。只吃裸正文；带 [`AFFIX_HEAD`] / [`AFFIX_TAIL`] 的主流完整串交给 [`decode`]。
pub fn decode_body(beast_text: &str) -> Result<String, DecodeError> {
    BeastDict::DEFAULT.decode_body(beast_text)
}

/// 一个码元的 4 个十六进制位，高位在前（等价于 `unit.toString(16)` 左补零到 4 位）
fn hex4(unit: u16) -> [usize; 4] {
    [
        (unit >> 12) as usize,
        (unit >> 8) as usize % 16,
        (unit >> 4) as usize % 16,
        unit as usize % 16,
    ]
}

/// 数学取模：Rust 的 `%` 跟随被除数符号，这里需要非负结果
fn rem(mut value: isize, modulus: isize) -> isize {
    value %= modulus;
    if value < 0 {
        value += modulus;
    }
    value
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// 兽语字符数为奇数，无法两两成组
    OddLength { char_count: usize },
    /// 出现了不在字典中的字符
    UnknownChar(char),
    /// 缺少 [`AFFIX_HEAD`] + [`AFFIX_TAIL`]，不是主流完整串
    MissingAffix,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::OddLength { char_count } => {
                write!(f, "兽语长度为 {char_count}，不是 2 的整数倍")
            }
            DecodeError::UnknownChar(ch) => write!(f, "字符 {ch:?} 不在兽语字典中"),
            DecodeError::MissingAffix => write!(
                f,
                "缺少主流附加头尾 {AFFIX_HEAD:?}…{AFFIX_TAIL:?}；裸正文请交给 decode_body"
            ),
        }
    }
}

impl std::error::Error for DecodeError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主流格式 = 字典序号 `4 + 2 + 1` + 正文 + `3`；裸正文与 `beast.js` 的 `encode` 同码
    #[test]
    fn readme_vectors() {
        assert_eq!(encode_body("你好"), "呜嗷嗷嗷啊嗷嗷~啊呜~啊~呜呜嗷");
        assert_eq!(
            decode_body("呜嗷嗷嗷啊嗷嗷~啊呜~啊~呜呜嗷").unwrap(),
            "你好"
        );
        assert_eq!(
            encode("你好"),
            "~呜嗷呜嗷嗷嗷啊嗷嗷~啊呜~啊~呜呜嗷啊" // = AFFIX_HEAD + 正文 + AFFIX_TAIL
        );
        assert_eq!(
            decode("~呜嗷呜嗷嗷嗷啊嗷嗷~啊呜~啊~呜呜嗷啊").unwrap(),
            "你好"
        );
    }

    /// 头尾由字典的 1 基序号拼出，不靠手抄：嗷=1 呜=2 啊=3 ~=4 → 头 = 4+2+1，尾 = 3
    #[test]
    fn affix_is_the_dictionary_numbers_4_2_1_and_3() {
        let nth = |i: usize| BEAST[i - 1];
        let head: String = [nth(4), nth(2), nth(1)].into_iter().collect();
        assert_eq!(AFFIX_HEAD, head);
        assert_eq!(AFFIX_TAIL, nth(3).to_string());
    }

    /// 裸正文与主流串解出同一结果；裸正文交给 `decode` 会报缺头尾，而不是被静默错位解码
    #[test]
    fn mainstream_and_body_agree() {
        for text in ["你好", "甲乙丙丁一二三四", "\u{1F600}", ""] {
            let wrapped = encode(text);
            assert!(wrapped.starts_with(AFFIX_HEAD) && wrapped.ends_with(AFFIX_TAIL));
            assert_eq!(decode(&wrapped).unwrap(), text, "input={text:?}");
            assert_eq!(
                decode(&wrapped).unwrap(),
                decode_body(&encode_body(text)).unwrap()
            );
        }
        assert_eq!(
            decode(&encode_body("你好")).unwrap_err(),
            DecodeError::MissingAffix
        );
    }

    /// 正文自己就能长得像"带头尾的主流串"，所以头尾绝不靠猜：
    /// `퀃`(U+D003) 的正文是 `~呜嗷呜嗷啊呜啊`，按长度 4 剥掉会让整条位流挪 2 位
    #[test]
    fn affix_is_never_guessed() {
        let bare = encode_body("\u{D003}");
        assert!(bare.starts_with(AFFIX_HEAD) && bare.ends_with(AFFIX_TAIL));
        assert_eq!(decode_body(&bare).unwrap(), "\u{D003}");
        assert_ne!(decode(&bare).unwrap(), "\u{D003}");
    }

    #[test]
    fn round_trip_ascii() {
        for text in ["a", "ab", "hello world", "0123456789", " ", "!@#$%"] {
            assert_eq!(decode(&encode(text)).unwrap(), text, "input={text:?}");
        }
    }

    #[test]
    fn round_trip_cjk() {
        let samples = [
            "你好",
            "兽音译者",
            "日本語テキスト",
            "한국어",
            "你好世界，hello 123！",
        ];
        for text in samples {
            assert_eq!(decode(&encode(text)).unwrap(), text, "input={text:?}");
        }
    }

    /// 滚动键每 16 个十六进制位（= 4 个码元）回绕，跨回绕点必须仍然可逆
    #[test]
    fn round_trip_across_key_wrap() {
        let text = "兽音译者SDK测试样例0123456789";
        assert_eq!(decode(&encode(text)).unwrap(), text);
        let long = "x".repeat(200);
        assert_eq!(decode(&encode(&long)).unwrap(), long);
    }

    /// 定长 4 位让位流永远对齐，所以控制字符、2~3 位码点、辅助平面全都无损
    /// ——这正是 Python 端丢掉的那三类
    #[test]
    fn round_trip_lossless_everywhere() {
        let samples = [
            "\u{01}\u{02}",                                // Python 解成 U+0012
            "\t",                                          // Python 只写 1 位
            "\u{00E9}",                                    // 2 位码点
            "\u{07E0}",                                    // 3 位码点
            "\u{1F600}",                                   // 代理对
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", // ZWJ 序列
            "\u{10FFFF}",                                  // 码点上界
        ];
        for text in samples {
            assert_eq!(decode(&encode(text)).unwrap(), text, "input={text:?}");
            assert_eq!(
                decode_body(&encode_body(text)).unwrap(),
                text,
                "input={text:?}"
            );
        }
    }

    /// 一个辅助平面字符 = 两个码元 = 8 位 = 16 个兽语字（Python 端只有 10 个）
    #[test]
    fn astral_uses_surrogate_pairs() {
        assert_eq!(encode("\u{1F600}"), "~呜嗷~呜啊呜呜呜嗷嗷嗷呜嗷~呜啊呜~啊");
        assert_eq!(encode_body("\u{1F600}"), "~呜啊呜呜呜嗷嗷嗷呜嗷~呜啊呜~");
        assert_eq!(decode(&encode("\u{1F600}")).unwrap(), "\u{1F600}");
    }

    #[test]
    fn empty_input() {
        assert_eq!(encode_body(""), "");
        assert_eq!(decode_body("").unwrap(), "");
        assert_eq!(encode(""), format!("{AFFIX_HEAD}{AFFIX_TAIL}"));
        assert_eq!(decode(&encode("")).unwrap(), "");
    }

    #[test]
    fn decode_rejects_bad_input() {
        assert_eq!(
            decode_body("呜嗷嗷").unwrap_err(),
            DecodeError::OddLength { char_count: 3 }
        );
        assert_eq!(
            decode_body("呜喵").unwrap_err(),
            DecodeError::UnknownChar('喵')
        );
        assert_eq!(decode("呜嗷嗷").unwrap_err(), DecodeError::MissingAffix);
    }

    /// 末尾凑不满一个码元的位按 JS 的行为丢掉，不报错
    #[test]
    fn decode_drops_incomplete_tail_unit() {
        let mut core: Vec<char> = encode_body("你好").chars().collect();
        core.pop();
        assert_eq!(
            decode_body(&core.iter().collect::<String>()).unwrap_err(),
            DecodeError::OddLength { char_count: 15 }
        );
        core.pop(); // 7 个十六进制位 → 只够 1 个码元
        assert_eq!(decode_body(&core.iter().collect::<String>()).unwrap(), "你");
    }

    /// 自定义字典：头尾按 1 基序号 4+2+1 / 3 随字典一起换
    #[test]
    fn custom_dict_affix_follows_dictionary() {
        let dict = BeastDict::parse("αβγδ").unwrap();
        assert_eq!(dict.head(), "δβα"); // 第4=δ 第2=β 第1=α
        assert_eq!(dict.tail(), 'γ'); // 第3
        let full = dict.encode("你好😀");
        assert!(full.starts_with("δβα") && full.ends_with('γ'));
        assert_eq!(dict.decode(&full).unwrap(), "你好😀");
    }

    /// 默认字典的 BeastDict 方法与旧自由函数逐字符一致（增量不破坏既有语义）
    #[test]
    fn default_dict_methods_agree_with_free_functions() {
        for text in ["", "你好", "a\tb\u{1F600}", "甲乙丙丁一二三四"] {
            assert_eq!(BeastDict::DEFAULT.encode(text), encode(text));
            assert_eq!(BeastDict::DEFAULT.encode_body(text), encode_body(text));
            assert_eq!(
                BeastDict::DEFAULT.decode(&encode(text)).unwrap(),
                decode(&encode(text)).unwrap()
            );
        }
        assert_eq!(BeastDict::DEFAULT.head(), AFFIX_HEAD);
        assert_eq!(BeastDict::DEFAULT.tail().to_string(), AFFIX_TAIL);
    }

    /// 不同字典的完整串互不通用：拿默认字典解自定义字典的串必须响亮报缺头尾
    #[test]
    fn cross_dict_decode_rejected() {
        let dict = BeastDict::parse("一二三四").unwrap();
        let full = dict.encode("你好");
        assert_eq!(decode(&full).unwrap_err(), DecodeError::MissingAffix);
    }

    #[test]
    fn dict_validation() {
        assert_eq!(
            BeastDict::parse("aaa"),
            Err(DictError::WrongLength { char_count: 3 })
        );
        assert_eq!(
            BeastDict::parse("嗷呜啊~x").unwrap_err(),
            DictError::WrongLength { char_count: 5 }
        );
        assert_eq!(
            BeastDict::parse("嗷呜啊嗷"),
            Err(DictError::DuplicateChar('嗷'))
        );
        assert!(BeastDict::parse("呜啊~嗷").is_ok()); // 换序合法，头尾随之变
    }
}
