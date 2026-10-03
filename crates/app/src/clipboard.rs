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

//! 系统剪贴板。
//!
//! 非走不可自己接：`egui-winit 0.31` 的 `clipboard.rs` 把 arboard 整段挂在
//! `#[cfg(all(feature = "arboard", not(target_os = "android")))]` 上，安卓编译时直接排除，
//! 于是 egui 里的复制只落进它那个「只在同一个 app 内有效」的兜底 `String`，
//! 复制到别的 app 会粘出旧内容或空。桌面这条路本来是好的，所以桌面继续用 arboard，
//! 只是改成自己调，好让长按菜单的两个平台行为一致。

#[cfg(target_os = "android")]
pub fn copy(text: &str) {
    crate::android::clipboard_set(text);
}

#[cfg(not(target_os = "android"))]
pub fn copy(text: &str) {
    if let Err(e) = arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_owned())) {
        log::error!("复制到系统剪贴板失败: {e}");
    }
}

/// 读系统剪贴板；空或取不到都回 `None`（调用方按"没东西可粘"处理）。
#[cfg(target_os = "android")]
pub fn paste() -> Option<String> {
    crate::android::clipboard_get()
}

#[cfg(not(target_os = "android"))]
pub fn paste() -> Option<String> {
    match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
        Ok(text) if !text.is_empty() => Some(text),
        Ok(_) => None,
        Err(e) => {
            log::error!("读系统剪贴板失败: {e}");
            None
        }
    }
}
