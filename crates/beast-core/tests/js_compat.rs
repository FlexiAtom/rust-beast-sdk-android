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

//! 跨语言一致性。两份表都用 hex 字段，不受字典字符（含 `~`）与 TAB 影响：
//! - `tests/vectors.tsv` 由参考实现 `JavaScript/beast.js` 生成（`node Rust/tests/gen_vectors.js`
//!   重生成），3359 条，覆盖 ASCII / CJK / 1~3 位码点 / 辅助平面 / 控制字符 / 混排 / 滚动键回绕。
//!   表里存的是**裸正文**，对应 [`beast::encode_body`] / [`beast::decode_body`]。
//! - `tests/mainstream.tsv` 是项目所有者从主流兽音译者实测拿到的 11 条完整串原文，未经转录。

struct Vector {
    input: String,
    js_encode: String,
    js_roundtrip: String,
    /// 参考实现的往返含孤立代理——Rust 字符串无法表示，比对时单列（现表为 0 条）
    js_has_lone_surrogate: bool,
}

fn from_hex(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("非法 hex"))
        .collect()
}

fn utf8(hex: &str) -> String {
    String::from_utf8(from_hex(hex)).expect("非 UTF-8")
}

/// JS 的往返结果是一段 UTF-16 码元；孤立代理在 Rust 模型里对应 U+FFFD
fn utf16le_to_rust_model(hex: &str) -> String {
    let bytes = from_hex(hex);
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    char::decode_utf16(units)
        .map(|u| u.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

fn read_table(name: &str) -> Vec<String> {
    let path = format!("{}/tests/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path)
        .expect("读取表失败")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

fn vectors() -> Vec<Vector> {
    read_table("vectors.tsv")
        .iter()
        .filter_map(|line| {
            let mut it = line.split('\t');
            let (input, js_encode, js_roundtrip, surrogate) =
                (it.next()?, it.next()?, it.next()?, it.next()?);
            Some(Vector {
                input: utf8(input),
                js_encode: utf8(js_encode),
                js_roundtrip: js_roundtrip.to_owned(),
                js_has_lone_surrogate: surrogate == "1",
            })
        })
        .collect()
}

#[test]
fn encode_body_matches_javascript_character_for_character() {
    let vs = vectors();
    let mismatches: Vec<&Vector> = vs
        .iter()
        .filter(|v| beast::encode_body(&v.input) != v.js_encode)
        .collect();
    assert!(
        mismatches.is_empty(),
        "encode_body 与 JavaScript 不一致 {} / {} 条:\n{}",
        mismatches.len(),
        vs.len(),
        mismatches
            .iter()
            .take(5)
            .map(|v| format!(
                "  input={:?}\n    js={:?}\n    rs={:?}",
                v.input,
                v.js_encode,
                beast::encode_body(&v.input)
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(vs.len() > 3000, "向量不足: {}", vs.len());
}

/// [`beast::encode`] = 主流格式 `4 + 2 + 1` + 正文 + `3`
#[test]
fn encode_wraps_the_javascript_body_in_the_mainstream_affix() {
    let vs = vectors();
    let mismatches: Vec<&Vector> = vs
        .iter()
        .filter(|v| {
            beast::encode(&v.input)
                != format!("{}{}{}", beast::AFFIX_HEAD, v.js_encode, beast::AFFIX_TAIL)
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "encode 与「头 + JS 正文 + 尾」不一致 {} / {} 条:\n{}",
        mismatches.len(),
        vs.len(),
        mismatches
            .iter()
            .take(5)
            .map(|v| format!(
                "  input={:?}\n    期望={:?}\n    实得={:?}",
                v.input,
                format!("{}{}{}", beast::AFFIX_HEAD, v.js_encode, beast::AFFIX_TAIL),
                beast::encode(&v.input)
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn decode_body_matches_javascript() {
    let vs = vectors();
    let mismatches: Vec<&Vector> = vs
        .iter()
        .filter(|v| !v.js_has_lone_surrogate)
        .filter(|v| {
            beast::decode_body(&v.js_encode).unwrap() != utf16le_to_rust_model(&v.js_roundtrip)
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "decode_body 与 JavaScript 不一致 {} / {} 条:\n{}",
        mismatches.len(),
        vs.len(),
        mismatches
            .iter()
            .take(5)
            .map(|v| format!(
                "  input={:?}\n    js={:?}\n    rs={:?}",
                v.input,
                utf16le_to_rust_model(&v.js_roundtrip),
                beast::decode_body(&v.js_encode).unwrap()
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// JS 端定长 4 位使位流永远对齐，所以**每一条**向量都应往返无损（Python 端做不到）
#[test]
fn round_trip_is_lossless_for_every_vector() {
    let vs = vectors();
    let failures: Vec<&Vector> = vs
        .iter()
        .filter(|v| !v.input.is_empty())
        .filter(|v| beast::decode_body(&beast::encode_body(&v.input)).unwrap() != v.input)
        .collect();
    assert!(
        failures.is_empty(),
        "往返有损 {} 条:\n{}",
        failures.len(),
        failures
            .iter()
            .take(5)
            .map(|v| format!(
                "  input={:?}\n    out={:?}",
                v.input,
                beast::decode_body(&beast::encode_body(&v.input)).unwrap()
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn mainstream_form_round_trips_for_every_vector() {
    let failures: Vec<String> = vectors()
        .into_iter()
        .map(|v| v.input)
        .filter(|input| beast::decode(&beast::encode(input)).unwrap() != *input)
        .collect();
    assert!(failures.is_empty(), "主流完整串往返失败: {failures:?}");
}

/// 反向证据：向量表里确实存在"裸正文长得像主流串"的输入，所以 [`beast::decode`] 不猜头尾
#[test]
fn some_bare_bodies_look_like_mainstream_strings() {
    let look_alikes: Vec<String> = vectors()
        .into_iter()
        .map(|v| beast::encode_body(&v.input))
        .filter(|body| body.starts_with(beast::AFFIX_HEAD) && body.ends_with(beast::AFFIX_TAIL))
        .collect();
    assert!(
        !look_alikes.is_empty(),
        "应当存在撞上车头车尾的正文（如 U+D003）"
    );
}

/// 主流实测：完整串 = `~呜嗷` + 正文 + `啊`，11 条样本逐字符命中
#[test]
fn mainstream_observed_samples() {
    let rows = read_table("mainstream.tsv");
    assert_eq!(rows.len(), 11, "样本数与来源记录不符: {}", rows.len());
    for row in &rows {
        let (input, observed) = row.split_once('\t').expect("mainstream.tsv 字段缺失");
        let (input, observed) = (utf8(input), utf8(observed));
        assert_eq!(
            beast::encode(&input),
            observed,
            "encode 与主流实测不一致: input={input:?}"
        );
        assert_eq!(
            beast::decode(&observed).unwrap(),
            input,
            "decode 主流实测串失败: input={input:?}"
        );
        // 同一条串也要能被"从串里提字典"的那条路解开，提出来的正是默认字典。
        // 这条路不能只在 Rust 自己编出来的串上成立——那等于只验证了逆命题。
        let (text, dict, dropped) = beast::decode_mainstream(&observed).unwrap();
        assert_eq!(
            text, input,
            "decode_mainstream 主流实测串失败: input={input:?}"
        );
        assert_eq!(
            dict,
            beast::BeastDict::DEFAULT,
            "实测串提出来应当是默认字典"
        );
        assert_eq!(dropped, 0, "实测串不该有残缺尾部: {observed:?}");
    }
}
