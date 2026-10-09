#!/bin/sh
# Quit the running Emaki and start the build in target/debug, a few
# seconds from now, and return at once. For an agent working inside
# Emaki: the session is a child of the app, so the app cannot be quit
# while the turn is still being written.
#
#   scripts/relaunch.sh [seconds]     default 15
#
# The wait is the countdown and then a state, not a longer time: Claude
# Code's registry record for this session (~/.claude/sessions/<pid>.json)
# says `busy` for as long as the turn runs, and nothing is quit until it
# no longer does.

cd "$(dirname "$0")/.." || exit 1
delay="${1:-15}"

# The session this was run from: the nearest ancestor with a registry
# record. Found now, while the ancestors are still there to walk.
rec=""
p=$$
while [ "${p:-1}" -gt 1 ]; do
    if [ -f "$HOME/.claude/sessions/$p.json" ]; then
        rec="$HOME/.claude/sessions/$p.json"
        break
    fi
    p=$(ps -o ppid= -p "$p" 2>/dev/null | tr -d ' ')
done

REC="$rec" DELAY="$delay" nohup sh -c '
    sleep "$DELAY"
    if [ -n "$REC" ]; then
        n=0
        while grep -q "\"status\":\"busy\"" "$REC" 2>/dev/null && [ "$n" -lt 900 ]; do
            sleep 1
            n=$((n + 1))
        done
        # Claude Code writes the last rows of a turn a moment after it
        # says idle.
        [ "$n" -gt 0 ] && sleep 2
    fi
    pkill -x Emaki
    sleep 2
    exec scripts/dev-app.sh
' >/dev/null 2>&1 &
echo "Emaki relaunches ${delay}s from now, or when this turn ends if that is later"
