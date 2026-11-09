#!/usr/bin/env bash
# Usage: ./generate.sh "<style prompt>" [lyrics] [duration_s]
set -euo pipefail
API=${ACESTEP_URL:-http://127.0.0.1:8001}
PROMPT=${1:?style prompt required}
LYRICS=${2:-}
DURATION=${3:-60}
OUT=${OUT_DIR:-$(dirname "$0")/output}
mkdir -p "$OUT"

body=$(jq -n --arg p "$PROMPT" --arg l "$LYRICS" --argjson d "$DURATION" \
  '{prompt:$p, lyrics:$l, audio_duration:$d, thinking:true, batch_size:1}')
task=$(curl -sf "$API/release_task" -H 'Content-Type: application/json' -d "$body" | jq -r '.data.task_id')
echo "task: $task"

while :; do
  res=$(curl -sf "$API/query_result" -H 'Content-Type: application/json' -d "{\"task_id_list\":[\"$task\"]}")
  status=$(jq -r '.data[0].status' <<<"$res")
  [ "$status" = 1 ] && break
  [ "$status" = 2 ] && { echo "failed: $res" >&2; exit 1; }
  sleep 3
done

jq -r '.data[0].result' <<<"$res" | jq -r '.[].file' | while read -r f; do
  [ -n "$f" ] || { echo "task succeeded but no audio file; check server log" >&2; exit 1; }
  name="$OUT/$(date +%Y%m%d-%H%M%S)-${task:0:8}.${f##*.}"
  curl -sf "$API$f" -o "$name" && echo "saved: $name"
done
