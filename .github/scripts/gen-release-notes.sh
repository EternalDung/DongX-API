#!/usr/bin/env bash
#
# 生成 Release 更新日志。
#
# 用法:
#   .github/scripts/gen-release-notes.sh v1.6.0            # 打印到 stdout
#   .github/scripts/gen-release-notes.sh v1.6.0 notes.md   # 写入文件
#
# 原理:
#   取「上一个 tag .. 当前 tag」区间内的 commit，按约定式提交前缀分成四类。
#   输出文本有两个去处，且同源：
#     ① GitHub Release 正文（release.yml 的 releaseBody）
#     ② tauri-action 会写进 latest.json 的 notes → 应用内「发现新版本」弹窗的更新说明
#
# 前提:
#   checkout 需 fetch-depth: 0，否则本地没有历史 tag，只能退化成 HEAD 区间。
#
# 本地预览:
#   bash .github/scripts/gen-release-notes.sh v1.5.2
#
set -euo pipefail

TAG="${1:-}"
OUT="${2:-}"

if [ -z "${TAG}" ]; then
  echo "用法: $0 <tag> [输出文件]" >&2
  exit 1
fi

# ---------- 1. 定位比较区间 ----------
TARGET="HEAD"
PREV=""

if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null 2>&1; then
  TARGET="${TAG}"
  # 当前 tag 之前最近的一个 tag（^ 用于排除自身）
  PREV="$(git describe --tags --abbrev=0 "${TAG}^" 2>/dev/null || true)"
else
  echo "提示: 本地未找到 tag '${TAG}'，改为按 HEAD 生成。" >&2
  PREV="$(git describe --tags --abbrev=0 HEAD 2>/dev/null || true)"
fi

if [ -n "${PREV}" ]; then
  RANGE="${PREV}..${TARGET}"
else
  RANGE="${TARGET}"
fi

# ---------- 2. 抓取 commit 并按类型归类 ----------
CLASSIFIED="$(
  git log --no-merges --pretty=format:'%s' "${RANGE}" 2>/dev/null | awk '
    # 解析约定式提交前缀，返回「去掉前缀的正文」，类型写进全局 TYPE。
    # 全角冒号/括号也用 index() 处理：index() 在按字节和按字符的 awk 里
    # 行为一致，而 sub() 的 \1 反向引用是 gawk 扩展，mawk 不可移植。
    function take_type(s,   head, rest, p1, p2) {
      TYPE = ""
      if (!match(s, /^[a-zA-Z]+/)) return s      # 开头不是 ASCII 类型词 -> 非规范提交
      head = substr(s, 1, RLENGTH)
      rest = substr(s, RLENGTH + 1)

      # 可选作用域: feat(ui): / feat（ui）:
      if (index(rest, "(") == 1 || index(rest, "（") == 1) {
        p1 = index(rest, ")")
        p2 = index(rest, "）")
        if (p1 > 0 && (p2 == 0 || p1 < p2)) {
          rest = substr(rest, p1 + 1)
        } else if (p2 > 0) {
          rest = substr(rest, p2 + length("）"))
        } else {
          return s                              # 括号没闭合，按非规范提交处理
        }
      }

      if (index(rest, "!") == 1) rest = substr(rest, 2)   # 破坏性变更标记
      if (index(rest, ":") == 1) {                        # 半角冒号
        TYPE = head
        return substr(rest, 2)
      }
      if (index(rest, "：") == 1) {                       # 全角冒号
        TYPE = head
        return substr(rest, 1 + length("："))
      }
      return s
    }

    NF == 0 { next }
    {
      TYPE = ""
      subj = take_type($0)
      sub(/^[[:space:]]+/, "", subj)
      t = tolower(TYPE)
      if (t == "feat")                               k = "feat"
      else if (t == "fix")                           k = "fix"
      else if (t == "refactor" || t == "perf")       k = "opt"
      else                                           k = "misc"
      print k "\t" subj
    }'
)"

FEAT=""
FIX=""
OPT=""
MISC=""

while IFS=$'\t' read -r kind subject; do
  if [ -n "${kind:-}" ] && [ -n "${subject:-}" ]; then
    case "${kind}" in
      feat) FEAT="${FEAT}- ${subject}"$'\n' ;;
      fix)  FIX="${FIX}- ${subject}"$'\n' ;;
      opt)  OPT="${OPT}- ${subject}"$'\n' ;;
      *)    MISC="${MISC}- ${subject}"$'\n' ;;
    esac
  fi
done <<< "${CLASSIFIED}"

# ---------- 3. 组装 Markdown ----------
# 注意: 这段文本也会原样出现在应用内的更新弹窗里，所以语法只用
# 「### 标题 / - 列表 / **加粗** / `代码` / --- 分隔线」这几种，
# 前端 UpdateDialog 有对应的极简渲染。
section() { # $1=标题 $2=条目（已带换行）
  if [ -n "${2}" ]; then
    printf '### %s\n\n%s\n' "${1}" "${2}"
  fi
}

build() {
  if [ -z "${FEAT}${FIX}${OPT}${MISC}" ]; then
    printf '本次发布没有代码变更（可能只是重新打包）。\n\n'
  else
    section "✨ 新功能" "${FEAT}"
    section "🐛 问题修复" "${FIX}"
    section "♻️ 重构与优化" "${OPT}"
    section "🔧 其他改动" "${MISC}"
  fi

  printf -- '---\n\n**安装说明**\n\n'
  printf -- '- Windows：运行 `-setup.exe`；出现 SmartScreen 提示时点「更多信息」→「仍要运行」。\n'
  printf -- '- macOS：安装包未签名，首次启动先在终端执行 `xattr -cr /Applications/DongX.app`，再从 Finder 打开。\n'
  printf -- '- Linux：使用 `.AppImage`（`chmod +x` 后双击）或 `.deb`（`sudo dpkg -i` 安装）。\n'
}

if [ -n "${OUT}" ]; then
  build > "${OUT}"
  echo "已写入 ${OUT}" >&2
else
  build
fi
