#!/bin/sh
# doclint.sh — docs/ 配下の日本語設計文書の機械検査 (SSOT)
# 使い方: sh tools/doclint.sh docs/design/agent-sessions-basic-design.md [...]
# 出力契約: 各指摘を "[severity] path:line message" で出力し、末尾に
# "Critical N / High N / Medium N / Low N" を出力する。全て0なら exit 0。
#
# ルール表 (STYLE_GUIDE 相当。人間可読の根拠は japanese-writing-style スキル):
#   C1: コードブロック外のアスタリスク2つ強調 (**text**)
#   C2: 日本語文末のコロン (行末が「:」で終わり、直後がリストでない構文)
#   H1: 全角文字と半角英数の間の半角スペース (例: 「cmux が」)
#   M1: 「今後の対応予定」「対応予定」(断定回避表現へ言い換える)
#   M2: 「解像度を上げ」等のテック用語比喩
#   L1: 「散文」(本文・文章へ言い換える)

crit=0; high=0; med=0; low=0

for f in "$@"; do
  [ -f "$f" ] || { echo "[Critical] $f:0 file not found"; crit=$((crit+1)); continue; }

  # コードフェンス内を除外した本文を行番号付きで得る
  body=$(awk 'BEGIN{code=0} /^```/{code=1-code; next} code==0{printf "%d:%s\n", NR, $0}' "$f")

  # C1: **強調**
  hits=$(printf '%s\n' "$body" | grep -E '\*\*[^*]+\*\*' | head -20)
  if [ -n "$hits" ]; then
    printf '%s\n' "$hits" | while IFS= read -r l; do echo "[Critical] $f:${l%%:*} bold emphasis (**)"; done
    crit=$((crit + $(printf '%s\n' "$hits" | wc -l | tr -d ' ')))
  fi

  # C2: 全角文字を含む行の行末コロン
  hits=$(printf '%s\n' "$body" | grep -E '[ぁ-んァ-ヶ一-龠].*[:：]$' | head -20)
  if [ -n "$hits" ]; then
    printf '%s\n' "$hits" | while IFS= read -r l; do echo "[Critical] $f:${l%%:*} sentence ends with colon"; done
    crit=$((crit + $(printf '%s\n' "$hits" | wc -l | tr -d ' ')))
  fi

  # H1: 全角と半角英数の間の半角スペース (表の罫線行・見出し行は除外)
  hits=$(printf '%s\n' "$body" | grep -vE '^[0-9]+:(\||#|>)' | grep -E '([ぁ-んァ-ヶ一-龠]) ([A-Za-z0-9])|([A-Za-z0-9]) ([ぁ-んァ-ヶ一-龠])' | head -30)
  if [ -n "$hits" ]; then
    printf '%s\n' "$hits" | while IFS= read -r l; do echo "[High] $f:${l%%:*} space between zenkaku and ascii"; done
    high=$((high + $(printf '%s\n' "$hits" | wc -l | tr -d ' ')))
  fi

  # M1: 対応予定
  hits=$(printf '%s\n' "$body" | grep -E '(今後の対応予定|対応予定)' | head -10)
  if [ -n "$hits" ]; then
    printf '%s\n' "$hits" | while IFS= read -r l; do echo "[Medium] $f:${l%%:*} roadmap phrasing (対応予定)"; done
    med=$((med + $(printf '%s\n' "$hits" | wc -l | tr -d ' ')))
  fi

  # M2: 解像度メタファ (画面解像度など実物の解像度は本文書では扱わないため一律検出)
  hits=$(printf '%s\n' "$body" | grep -E '解像度' | head -10)
  if [ -n "$hits" ]; then
    printf '%s\n' "$hits" | while IFS= read -r l; do echo "[Medium] $f:${l%%:*} tech metaphor (解像度)"; done
    med=$((med + $(printf '%s\n' "$hits" | wc -l | tr -d ' ')))
  fi

  # L1: 散文
  hits=$(printf '%s\n' "$body" | grep -E '散文' | head -10)
  if [ -n "$hits" ]; then
    printf '%s\n' "$hits" | while IFS= read -r l; do echo "[Low] $f:${l%%:*} word 散文"; done
    low=$((low + $(printf '%s\n' "$hits" | wc -l | tr -d ' ')))
  fi
done

echo "Critical $crit / High $high / Medium $med / Low $low"
[ "$crit" -eq 0 ] && [ "$high" -eq 0 ] && [ "$med" -eq 0 ] && [ "$low" -eq 0 ]
