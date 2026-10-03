#!/usr/bin/env python3
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
"""为 androidx 依赖生成 R.java（AGP 的 sourcegen 等价物）。

aar 的 classes.jar 不含 R 类，apk 也没有合并这些资源，运行时一旦初始化
androidx.core.view.ViewCompat 就 NoClassDefFoundError: androidx/core/R$id。
这里按 aar 的 R.txt 补出类，整数值只当 View.setTag 的键用（平台只要求
key>>>24 >= 2），所以给全局唯一的 0x7f 号段即可，不必与真实资源表一致。
"""
import re
import sys
import zipfile
import keyword
from pathlib import Path

IDENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
PKG = re.compile(rb'package="([^"]+)"')

aars_dir, out_dir = Path(sys.argv[1]), Path(sys.argv[2])
# (pkg, type) -> {name: value}，pkg 内 type 内名字唯一
tables: dict[str, dict[str, dict[str, int]]] = {}
next_id = 0x7F010000

for aar in sorted(aars_dir.glob("*.aar")):
    with zipfile.ZipFile(aar) as z:
        try:
            rtxt = z.read("R.txt").decode()
        except KeyError:
            continue
        manifest = z.read("AndroidManifest.xml")
        m = PKG.search(manifest)
        if not m:
            sys.exit(f"{aar.name}: AndroidManifest.xml 里没有 package 属性")
        pkg = m.group(1).decode()
    for line in rtxt.splitlines():
        parts = line.split()
        # styleable 是 int[] 字段，本仓用到的类不碰它，跳过
        if len(parts) < 3 or parts[0] != "int" or parts[1] == "styleable":
            continue
        typ, name = parts[1], parts[2]
        if not IDENT.match(name) or keyword.iskeyword(name):
            continue
        table = tables.setdefault(pkg, {}).setdefault(typ, {})
        if name not in table:
            table[name] = next_id
            next_id += 1

count = 0
for pkg, types in tables.items():
    target = out_dir / pkg.replace(".", "/") / "R.java"
    target.parent.mkdir(parents=True, exist_ok=True)
    body = [f"package {pkg};", "", "public final class R {", "  private R() {}"]
    for typ in sorted(types):
        body.append(f"  public static final class {typ} {{")
        for name, value in sorted(types[typ].items()):
            body.append(f"    public static final int {name} = {hex(value)};")
            count += 1
        body.append("  }")
    body.append("}")
    target.write_text("\n".join(body) + "\n")
    print(f"生成 {target.relative_to(out_dir)}（{len(types)} 个类型）", file=sys.stderr)
print(f"R.java: {len(tables)} 个包 / {count} 个字段")
