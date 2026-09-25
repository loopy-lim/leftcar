#!/usr/bin/env bash
# site/scripts/subset-font.sh — Wanted Sans Bold(700) 커스텀 서브셋 재생성
#
# 근거: site/FONT-DECISION.md §3.1 (design.md §5 P3 해소)
# - 원본: Wanted Sans v1.0.3 (SIL OFL 1.1) — https://github.com/wanteddev/wanted-sans
# - 서브셋 문자집합: site 카피 사용 문자(src/content/site.ts + index.html) 전체
#   + 버퍼(ASCII, 호환 자모, 자주 쓰는 문장부호)
# - 카피를 대폭 변경했으면 이 스크립트를 다시 실행한다:
#     cd site && ./scripts/subset-font.sh
# - 결과: site/public/fonts/WantedSans-Bold.subset.woff2 (예산 50KB 이하)
set -euo pipefail

WANTED_SANS_VERSION="1.0.3"
WANTED_SANS_URL="https://github.com/wanteddev/wanted-sans/releases/download/v${WANTED_SANS_VERSION}/WantedSans-${WANTED_SANS_VERSION}.zip"
WANTED_SANS_OFL_URL="https://raw.githubusercontent.com/wanteddev/wanted-sans/v${WANTED_SANS_VERSION}/OFL.txt"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="$ROOT/public/fonts"
mkdir -p "$OUT_DIR"
FONT_NAME="WantedSans-Bold"
SUBSET_OUT="$OUT_DIR/${FONT_NAME}.subset.woff2"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

command -v python3 >/dev/null || { echo "python3 필요"; exit 1; }
python3 -c "import fontTools, brotli" 2>/dev/null || { echo "fontTools + brotli 필요: pip3 install fonttools brotli"; exit 1; }

# 1) 원본 다운로드 (로컬 zip 재사용: WANTED_SANS_ZIP=/path/to.zip ./scripts/subset-font.sh)
ZIP="${WANTED_SANS_ZIP:-$TMP/WantedSans-${WANTED_SANS_VERSION}.zip}"
if [ ! -f "$ZIP" ]; then
  echo ">> 다운로드: $WANTED_SANS_URL"
  curl -sSL -o "$ZIP" "$WANTED_SANS_URL"
fi
unzip -o -q "$ZIP" "webfonts/static/complete/woff2/${FONT_NAME}.woff2" -d "$TMP"
SRC_WOFF2="$TMP/webfonts/static/complete/woff2/${FONT_NAME}.woff2"

# 2) 서브셋 문자집합 생성: 사이트 카피 사용 문자 + 버퍼
python3 - "$ROOT" "$TMP/charset.txt" <<'PY'
import glob, os, sys

root, out_path = sys.argv[1], sys.argv[2]
sources = [os.path.join(root, "index.html")]
sources += sorted(glob.glob(os.path.join(root, "src", "content", "*.ts")))

used = set()
for path in sources:
    with open(path, encoding="utf-8") as f:
        used.update(f.read())

# 버퍼: 인쇄 가능 ASCII, 자주 쓰는 문장부호/기호, 한글 호환 자모 U+3131–U+3163
buffer = {chr(c) for c in range(0x20, 0x7F)}
buffer |= set("·—–‘’“”…!?',.:;()[]{}<>«»《》【】%‰+−×÷=≠≈±°©®™✓→←↑↓")
buffer |= {chr(c) for c in range(0x3131, 0x3164)}

charset = used & {chr(c) for c in range(0x20, 0x11_000)} | buffer
charset.discard("\n"); charset.discard("\r"); charset.discard("\t")
with open(out_path, "w", encoding="utf-8") as f:
    f.write("".join(sorted(charset)))
hangul = sum(1 for ch in charset if 0xAC00 <= ord(ch) <= 0xD7A3)
print(f">> 문자집합: {len(charset)}자 (완성형 한글 {hangul}자 포함)")
PY

# 3) 서브셋 + woff2 인코딩
python3 -m fontTools.subset "$SRC_WOFF2" \
  --text-file="$TMP/charset.txt" \
  --flavor=woff2 \
  --output-file="$SUBSET_OUT" \
  --layout-features='*' \
  --name-IDs='*' \
  --notdef-outline

# 4) 검증: 사용 문자가 모두 커버되는지 + 파일 크기
python3 - "$TMP/charset.txt" "$SUBSET_OUT" <<'PY'
import sys
from fontTools.ttLib import TTFont

charset_path, font_path = sys.argv[1], sys.argv[2]
with open(charset_path, encoding="utf-8") as f:
    wanted = set(f.read())
cmap = TTFont(font_path).getBestCmap()
missing = sorted(ch for ch in wanted if ord(ch) not in cmap)
if missing:
    print(f"!! 커버 누락 {len(missing)}자: {''.join(missing)}")
    sys.exit(1)
print(f">> 검증 통과: 요청 {len(wanted)}자 모두 커버")
PY

curl -sSL -o "$OUT_DIR/OFL.txt" "$WANTED_SANS_OFL_URL"
SIZE=$(wc -c < "$SUBSET_OUT" | tr -d ' ')
echo ">> 완료: $SUBSET_OUT (${SIZE} bytes, 예산 51200 bytes)"
[ "$SIZE" -le 51200 ] || { echo "!! 예산(50KB) 초과 — 문자집합/웨이트 확인"; exit 1; }
