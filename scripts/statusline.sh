#!/bin/bash
# Emaki's status line for Claude Code.
#
# Claude Code hands the status-line command one JSON object on stdin per
# refresh: the context window in use and the account's five-hour and
# seven-day rate limits. Those limits reach no file a terminal session
# leaves behind, so this script does two things with them:
#
#   1. Leaves a copy in ~/.emaki/state/rate_limits.json (EMAKI_HOME
#      overrides ~/.emaki), written whole and renamed into place, so the
#      Emaki app can show the same numbers under its composer. Only when
#      that directory already exists: the app made it, and nothing here
#      makes directories. Beside it, state/context/<session>.json holds
#      that session as the terminal has it now: its context window's
#      size, the tokens in use, the model and the effort. Claude
#      Code reruns this script when any of those change. (The permission
#      mode is not in what it hands over.)
#   2. Prints the line the terminal shows:
#
#        Context 18% | 5h: 12% (3h20m) | 7d: 42% (4d6h)
#
# It never exits non-zero and never writes to stderr: a status-line
# command that fails only makes the line go blank, but the habit is
# worth keeping for anything Claude Code runs. Needs jq.
#
# The app installs it at ~/.emaki/bin/statusline.sh from the copy built
# into the binary (Settings, "Status line") and points Claude Code's
# `statusLine` setting at that path; the file in the repository is the
# source. Based on the status line in tw93/Waza (MIT), simplified: no
# cache and no high-water file.

exec 2>/dev/null

input=$(cat)

# 1. The handoff, before anything that could fail.
emaki_state="${EMAKI_HOME:-$HOME/.emaki}/state"
if [ -d "$emaki_state" ] && [ -n "$input" ]; then
  tmp="$emaki_state/rate_limits.json.tmp$$"
  if printf '%s' "$input" | jq -c '{rate_limits: (.rate_limits // {}), seen_at: now}' > "$tmp"; then
    mv -f "$tmp" "$emaki_state/rate_limits.json"
  else
    rm -f "$tmp"
  fi
fi

# The session's own context window, under its id. The transcript says how
# many tokens are in use but not how large the window is, and one model id
# comes in two sizes, so the app cannot work a percentage out alone. Only
# when the app has made the directory.
if [ -d "$emaki_state/context" ] && [ -n "$input" ]; then
  sid=$(printf '%s' "$input" | jq -r '.session_id // empty' | tr -cd 'A-Za-z0-9_-')
  if [ -n "$sid" ]; then
    tmp="$emaki_state/context/$sid.json.tmp$$"
    if printf '%s' "$input" | jq -c '{
      model: (.model.id // ""),
      effort: (.effort.level // ""),
      window: (.context_window.context_window_size // 0),
      used: ((.context_window.current_usage.input_tokens // 0)
        + (.context_window.current_usage.cache_creation_input_tokens // 0)
        + (.context_window.current_usage.cache_read_input_tokens // 0)),
      seen_at: now}' > "$tmp"; then
      mv -f "$tmp" "$emaki_state/context/$sid.json"
    else
      rm -f "$tmp"
    fi
  fi
fi

# 2. The line.
tab=$(printf '\t')
parsed=$(printf '%s' "$input" | jq -r '[
  ((.context_window.current_usage.input_tokens // 0)
   + (.context_window.current_usage.cache_creation_input_tokens // 0)
   + (.context_window.current_usage.cache_read_input_tokens // 0) | tostring),
  (.context_window.context_window_size // 0 | tostring),
  (.rate_limits.five_hour.used_percentage // null | if . then (. | round | tostring) else "null" end),
  (.rate_limits.five_hour.resets_at // 0 | (tonumber? // 0) | floor | tostring),
  (.rate_limits.seven_day.used_percentage // null | if . then (. | round | tostring) else "null" end),
  (.rate_limits.seven_day.resets_at // 0 | (tonumber? // 0) | floor | tostring)
] | @tsv')

IFS="$tab" read -r used_tokens window_size five_pct five_reset seven_pct seven_reset <<EOF
$parsed
EOF

RESET="\033[0m"
DIM="\033[2m"
GREEN="\033[32m"
YELLOW="\033[33m"
RED="\033[31m"
BLUE="\033[94m"
MAGENTA="\033[95m"

# Seconds until a Unix timestamp (whole seconds: jq floors it above, since
# a fraction's digits would run into the integer's), as "4h23m" or "1d21h";
# nothing once past.
format_reset() {
  local ts="$1"
  [ -z "$ts" ] && return
  local epoch now diff
  epoch=$(printf '%s' "$ts" | tr -dc '0-9')
  [ -z "$epoch" ] && return
  now=$(date +%s)
  diff=$((epoch - now))
  [ "$diff" -le 0 ] && return
  local mins=$(( diff / 60 ))
  local hours=$(( mins / 60 ))
  local days=$(( hours / 24 ))
  if [ "$days" -ge 1 ]; then
    printf "%dd%dh" "$days" $(( hours % 24 ))
  elif [ "$hours" -ge 1 ]; then
    printf "%dh%dm" "$hours" $(( mins % 60 ))
  else
    printf "%dm" "$mins"
  fi
}

ctx_pct=0
if [ "$window_size" -gt 0 ] 2>/dev/null; then
  ctx_pct=$(awk -v u="$used_tokens" -v t="$window_size" 'BEGIN { printf "%d", (u/t)*100 }')
fi
if [ "$ctx_pct" -ge 85 ] 2>/dev/null; then
  ctx_color="$RED"
elif [ "$ctx_pct" -ge 70 ] 2>/dev/null; then
  ctx_color="$YELLOW"
else
  ctx_color="$GREEN"
fi
context_part="${DIM}Context${RESET} ${ctx_color}${ctx_pct}%${RESET}"

usage_color() {
  local pct="$1"
  if [ "$pct" -ge 90 ] 2>/dev/null; then printf "%s" "$RED"
  elif [ "$pct" -ge 70 ] 2>/dev/null; then printf "%s" "$MAGENTA"
  else printf "%s" "$BLUE"
  fi
}

# One window: "5h: 12% (3h20m)", or "5h: --" when the API sent none.
window_part() {
  local label="$1" pct="$2" reset="$3"
  if [ "$pct" != "null" ] && [ -n "$pct" ]; then
    local color reset_str
    color=$(usage_color "$pct")
    reset_str=$(format_reset "$reset")
    if [ -n "$reset_str" ]; then
      printf "%s" "${DIM}${label}:${RESET} ${color}${pct}%${RESET} ${DIM}(${reset_str})${RESET}"
    else
      printf "%s" "${DIM}${label}:${RESET} ${color}${pct}%${RESET}"
    fi
  else
    printf "%s" "${DIM}${label}: --${RESET}"
  fi
}

five_part=$(window_part 5h "$five_pct" "$five_reset")
seven_part=$(window_part 7d "$seven_pct" "$seven_reset")

printf "%b | %b | %b\n" "$context_part" "$five_part" "$seven_part"
exit 0
