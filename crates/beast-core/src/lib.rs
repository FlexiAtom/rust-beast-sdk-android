// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Copyright (C) 2026 FlexiAtom
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU Affero General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option) any
// later version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT ANY
// WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
// PARTICULAR PURPOSE. See the GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License along
// with this program. If not, see <https://www.gnu.org/licenses/>.

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
//!   这里照搬：不报错也不补位，手写残缺串会静默少解最后一个码元。要告诉用户少解了多少，
//!   用 [`BeastDict::decode_with_tail`] / [`BeastDict::decode_body_with_tail`] 拿回丢弃计数。
//!
//! Rust 的 `char` 存不了孤立代理项，`String::from_utf16_lossy` 会把它们换成 U+FFFD。
//! 只有残缺/手写的编码串能解出孤立代理项——Rust 字符串本身不可能编出它们。

/// 字典表：一个十六进制位 `k`（0..15）编码为 `beast[k / 4]` + `beast[k % 4]`。
/// 换成自定义字典时须保证 4 个元素互不重复。
pub const BEAST: [char; 4] = ['嗷', '呜', '啊', '~'];

/// 字典基数：一个字典字符承载 log2(4) = 2 位，一对承载一个十六进制位
const DICT_BASE: usize = 4;

/// 一个 UTF-16 码元写成定长 4 位十六进制，即 4 个 nibble
const NIBBLES_PER_UNIT: usize = 4;

/// 一个 nibble 编码为一对字典字符
const CHARS_PER_NIBBLE: usize = 2;

/// 主流兽音译者附在正文前的头，即字典序号 `4 + 2 + 1`（[`BEAST`] 的第 4、2、1 个）。
/// [`encode`] 自动加上，[`decode`] 要求它在。
pub const AFFIX_HEAD: &str = "~呜嗷";

/// 主流兽音译者附在正文后的尾，即字典序号 `3`（见 [`AFFIX_HEAD`]）
pub const AFFIX_TAIL: &str = "啊";

/// 一套可替换的字典（4 个互不重复的字符，1 基序号 1..4）。
///
/// 主流格式的头尾**不是固定字符串，而是字典的 1 基序号**：头 = `4 + 2 + 1`、尾 = `3`。
/// 换字典后头尾必须跟着换，这正是它换字典之后仍兼容主流的机制——默认字典下
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

    /// 读出一条主流串**自带**的字典，并返回剥掉头尾后的裸正文。
    ///
    /// 主流头是字典 1 基序号 `4 + 2 + 1`、尾是 `3`，四个位置各出现一次，所以头尾四个字符
    /// 恰好把整套字典确定下来：`chars = [头[2], 头[1], 尾, 头[0]]`。这意味着解主流串**不需要
    /// 事先知道字典**——[`decode_with_tail`](Self::decode_with_tail) 那种"拿本地字典的头尾去
    /// 匹配"只兼容字典恰好相同的情况，不是真兼容。
    pub fn split_mainstream(beast_text: &str) -> Result<(Self, &str), MainstreamError> {
        let mut chars = beast_text.chars();
        let (Some(d4), Some(d2), Some(d1)) = (chars.next(), chars.next(), chars.next()) else {
            return Err(MainstreamError::TooShort {
                char_count: beast_text.chars().count(),
            });
        };
        let Some(d3) = chars.next_back() else {
            return Err(MainstreamError::TooShort { char_count: 3 });
        };
        let dict = Self::new([d1, d2, d3, d4]).map_err(MainstreamError::BadDict)?;
        let head_len = d4.len_utf8() + d2.len_utf8() + d1.len_utf8();
        Ok((
            dict,
            &beast_text[head_len..beast_text.len() - d3.len_utf8()],
        ))
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

    /// 完整串 → 人话；头尾必须是**本字典**算出的，缺任一即 [`DecodeError::MissingAffix`]。
    ///
    /// 事先不知道字典时别用它——用 [`split_mainstream`](Self::split_mainstream) /
    /// [`decode_mainstream`]，它们从串自己的头尾里读字典。
    pub fn decode(&self, beast_text: &str) -> Result<String, DecodeError> {
        Ok(self.decode_with_tail(beast_text)?.0)
    }

    /// 同 [`decode`](Self::decode)，但额外返回尾部**被丢弃的字符数**
    pub fn decode_with_tail(&self, beast_text: &str) -> Result<(String, usize), DecodeError> {
        let core = beast_text
            .strip_prefix(&self.head())
            .and_then(|body| body.strip_suffix(self.tail()))
            .ok_or(DecodeError::MissingAffix)?;
        self.decode_body_with_tail(core)
    }

    pub fn decode_body(&self, beast_text: &str) -> Result<String, DecodeError> {
        Ok(self.decode_body_with_tail(beast_text)?.0)
    }

    /// 同 [`decode_body`](Self::decode_body)，但额外返回尾部**被丢弃的字符数**。
    ///
    /// 一个 UTF-16 码元要 4 个十六进制位 = 8 个字典字符，末尾不足 8 个的按主流行为静默丢弃，
    /// 所以这个计数只可能是 0、2、4、6（奇数长度走 [`DecodeError::OddLength`]）。解码结果本身
    /// 与 [`decode_body`](Self::decode_body) 逐字符一致，只是把"丢了多少"交给调用方决定要不要提示。
    pub fn decode_body_with_tail(&self, beast_text: &str) -> Result<(String, usize), DecodeError> {
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
        let dropped = nibbles.len() % NIBBLES_PER_UNIT * CHARS_PER_NIBBLE;
        let units = nibbles
            .chunks(NIBBLES_PER_UNIT)
            .filter(|chunk| chunk.len() == NIBBLES_PER_UNIT)
            .map(|chunk| chunk.iter().fold(0u16, |acc, &digit| acc * 16 + digit))
            .collect::<Vec<u16>>();
        Ok((String::from_utf16_lossy(&units), dropped))
    }
}

/// 人话 → 主流兽音译者的完整串：[`AFFIX_HEAD`] + 正文 + [`AFFIX_TAIL`]
pub fn encode(text: &str) -> String {
    BeastDict::DEFAULT.encode(text)
}

/// 人话 → 裸正文（不带 [`AFFIX_HEAD`] / [`AFFIX_TAIL`]，与 `JavaScript/beast.js` 同码）
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

/// 裸正文 → 人话。只吃不带头尾的裸正文；带 [`AFFIX_HEAD`] / [`AFFIX_TAIL`] 的主流完整串交给 [`decode`]。
pub fn decode_body(beast_text: &str) -> Result<String, DecodeError> {
    BeastDict::DEFAULT.decode_body(beast_text)
}

/// 主流完整串 → 人话，**字典从串自己的头尾里提取**，不接受外部预设字典。
///
/// 返回 `(文本, 这条串实际用的字典, 尾部被丢弃的字符数)`。任何一套 4 字符字典编出来的主流串都解得开，
/// 包括 [`BeastDict::DEFAULT`] 之外的——那才是"兼容主流"该有的意思。
pub fn decode_mainstream(beast_text: &str) -> Result<(String, BeastDict, usize), MainstreamError> {
    let (dict, body) = BeastDict::split_mainstream(beast_text)?;
    let (text, dropped) = dict
        .decode_body_with_tail(body)
        .map_err(MainstreamError::Body)?;
    Ok((text, dict, dropped))
}

/// [`decode_mainstream`] / [`BeastDict::split_mainstream`] 的错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainstreamError {
    /// 不足 4 个字符，连 3 位头 + 1 位尾都凑不出来
    TooShort { char_count: usize },
    /// 头尾四个字符有重复 ⇒ 提不出合法字典，即这不像一条主流串
    BadDict(DictError),
    /// 字典提出来了，但裸正文解不动
    Body(DecodeError),
}

impl std::fmt::Display for MainstreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MainstreamError::TooShort { char_count } => write!(
                f,
                "主流串至少要 4 个字符（头 3 + 尾 1），收到 {char_count} 个"
            ),
            MainstreamError::BadDict(e) => {
                write!(f, "从头尾提不出字典：{e}（说明它不是一条主流串）")
            }
            MainstreamError::Body(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for MainstreamError {}

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
    /// 编码串长度为奇数，无法两两成组
    OddLength { char_count: usize },
    /// 出现了不在字典中的字符
    UnknownChar(char),
    /// 头尾与**本字典**算出的 [`BeastDict::head`] / [`BeastDict::tail`] 不一致，不是本字典的主流完整串
    MissingAffix,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::OddLength { char_count } => {
                write!(f, "编码串长度为 {char_count}，不是 2 的整数倍")
            }
            DecodeError::UnknownChar(ch) => write!(f, "字符 {ch:?} 不在字典中"),
            DecodeError::MissingAffix => write!(
                f,
                "头尾缺失或与当前字典不符（头应为该字典的 4+2+1 三字符、尾应为 3 号字符）；\
                 裸正文请交给 decode_body，不确定字典的主流串请交给 decode_mainstream"
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

    /// 一个辅助平面字符 = 两个码元 = 8 位 = 16 个字典字符（Python 端只有 10 个）
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

    /// `*_with_tail` 的文本结果与原函数逐字符一致，只多给出丢弃计数
    #[test]
    fn with_tail_counts_the_dropped_chars() {
        let full = encode_body("你好");
        let chars: Vec<char> = full.chars().collect();
        assert_eq!(chars.len(), 16);
        assert_eq!(
            BeastDict::DEFAULT.decode_body_with_tail(&full).unwrap(),
            ("你好".to_owned(), 0)
        );
        // 砍掉最后 2/4/6 个字符，第二个码元都凑不满：已读 8 字符，其余全部计入丢弃
        for cut in [2usize, 4, 6] {
            let truncated: String = chars[..chars.len() - cut].iter().collect();
            assert_eq!(
                BeastDict::DEFAULT
                    .decode_body_with_tail(&truncated)
                    .unwrap(),
                ("你".to_owned(), 8 - cut),
                "砍掉 {cut} 个字符"
            );
        }
        // 主流完整串：头尾剥掉后正文残缺，同样如实报数
        let truncated = format!(
            "{}{}{}",
            AFFIX_HEAD,
            chars[..14].iter().collect::<String>(),
            AFFIX_TAIL
        );
        assert_eq!(
            BeastDict::DEFAULT.decode_with_tail(&truncated).unwrap(),
            ("你".to_owned(), 6)
        );
        assert_eq!(BeastDict::DEFAULT.decode(&truncated).unwrap(), "你");
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

    /// 主流串自带字典：任何一套字典编出来的串，都在**不预设字典**的情况下解得回。
    /// 这才是"兼容主流"的意思——旧通路拿本地字典的头尾去匹配，跨字典直接拒。
    #[test]
    fn mainstream_string_carries_its_own_dictionary() {
        for source in ["", "你好", "a\tb\u{1F600}", "甲乙丙丁"] {
            for raw in ["αβγδ", "一二三四", "嗷呜啊~", "🐉🐍🐢🐟"] {
                let dict = BeastDict::parse(raw).unwrap();
                let (text, recovered, dropped) = decode_mainstream(&dict.encode(source)).unwrap();
                assert_eq!(text, source, "{raw:?} 往返");
                assert_eq!(recovered, dict, "提出来的字典应当就是编码时那套");
                assert_eq!(dropped, 0, "{raw:?}");
            }
        }
    }

    /// 旧严格通路语义未放宽：本地字典不匹配的主流串照样解不动
    #[test]
    fn strict_decode_still_rejects_foreign_dictionary() {
        let foreign = BeastDict::parse("αβγδ").unwrap().encode("你好");
        assert_eq!(
            BeastDict::DEFAULT.decode(&foreign).unwrap_err(),
            DecodeError::MissingAffix
        );
    }

    #[test]
    fn decode_mainstream_names_why_it_failed() {
        assert_eq!(
            decode_mainstream("").unwrap_err(),
            MainstreamError::TooShort { char_count: 0 }
        );
        assert_eq!(
            decode_mainstream("呜嗷啊").unwrap_err(),
            MainstreamError::TooShort { char_count: 3 }
        );
        // 头尾四个字符有重复 ⇒ 提不出字典
        assert_eq!(
            decode_mainstream("啊啊啊啊").unwrap_err(),
            MainstreamError::BadDict(DictError::DuplicateChar('啊'))
        );
        // 字典提得出来（默认那套），但正文含字典外字符
        assert_eq!(
            decode_mainstream("~呜嗷喵呜啊").unwrap_err(),
            MainstreamError::Body(DecodeError::UnknownChar('喵'))
        );
    }

    /// 尾部残缺计数在自带字典通路上同样有效（残缺只可能出在正文，头尾按位置取）
    #[test]
    fn decode_mainstream_reports_dropped_tail() {
        let dict = BeastDict::parse("一二三四").unwrap();
        let body: Vec<char> = dict.encode_body("你好").chars().collect();
        let mut cut: Vec<char> = dict.head().chars().collect();
        cut.extend(&body[..body.len() - 2]); // 留 14 个正文字符 = 7 个码元位，凑不满 4 个
        cut.push(dict.tail());
        let (text, recovered, dropped) =
            decode_mainstream(&cut.into_iter().collect::<String>()).expect("头尾完好就应当解得开");
        assert_eq!(text, "你");
        assert_eq!(dropped, 6);
        assert_eq!(recovered, dict);
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
