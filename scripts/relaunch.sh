#!/bin/sh
# Quit the running Emaki and start the build in target/debug, a few
# seconds from now, and return at once. For an agent working inside
# Emaki: the session is a child of the app, so the app cannot be quit
# while the turn is still being written.
#
#   scripts/relaunch.sh [seconds]     default 15
#   scripts/relaunch.sh --installed [seconds]
#
# With --installed what is started is /Applications/Emaki.app, not the
# build: the end of a release, when target/ has been deleted
# (scripts/release-clean.sh).
#
# The wait is the countdown and then a state, not a longer time: Claude
# Code's registry record for this session (~/.claude/sessions/<pid>.json)
# says `busy` for as long as the turn runs, and nothing is quit until it
# no longer does.

cd "$(dirname "$0")/.." || exit 1
installed=""
[ "$1" = --installed ] && { installed=1; shift; }
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

REC="$rec" DELAY="$delay" INSTALLED="$installed" nohup sh -c '
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
    [ -z "$INSTALLED" ] && exec scripts/dev-app.sh
    # As dev-app.sh opens the build: the environment a Dock launch has,
    # and none of the CLAUDE* variables of this session.
    exec env -i \
        HOME="$HOME" USER="$USER" LOGNAME="${LOGNAME:-$USER}" SHELL="${SHELL:-/bin/zsh}" \
        PATH=/usr/bin:/bin:/usr/sbin:/sbin \
        TMPDIR="$(getconf DARWIN_USER_TEMP_DIR)" \
        SSH_AUTH_SOCK="$(launchctl getenv SSH_AUTH_SOCK)" \
        /usr/bin/open -n /Applications/Emaki.app
' >/dev/null 2>&1 &
if [ -n "$installed" ]; then
    echo "The installed Emaki opens ${delay}s from now, or when this turn ends if that is later"
    exit 0
fi
echo "Emaki relaunches ${delay}s from now, or when this turn ends if that is later"
