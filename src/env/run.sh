#!/bin/sh
# In-pod runner for oat-agents.
#
#   run.sh start <exec-id> <cwd> <command...>
#   run.sh attach <exec-id>
#
# The command runs detached and writes its own exit code to a file, so a
# dropped exec stream costs the output it was carrying and nothing else: the
# work keeps going and `attach` picks the same execution back up.
set -u

mode=${1:-}
id=${2:-}
if [ -z "$mode" ] || [ -z "$id" ]; then
    echo "oat-run: usage: run.sh start|attach <exec-id> [...]" >&2
    exit 96
fi
shift 2

D="/tmp/oat-agents-exec/$id"
export D

case "$mode" in
    start)
        cwd=${1:-}
        if [ -z "$cwd" ]; then
            echo "oat-run: start needs a working directory" >&2
            exit 96
        fi
        shift
        if [ -e "$D/pid" ]; then
            echo "oat-run: exec $id was already started" >&2
            exit 97
        fi
        mkdir -p "$D" || exit 96
        : > "$D/out"
        : > "$D/err"
        cd "$cwd" || {
            echo "oat-run: cannot enter $cwd" >&2
            exit 96
        }
        if command -v setsid >/dev/null 2>&1; then
            detach=setsid
        else
            detach=""
        fi
        $detach /bin/sh -c '"$@" >"$D/out" 2>"$D/err"; printf %s $? >"$D/exit"' oat "$@" &
        echo $! > "$D/pid"
        ;;
    attach)
        if [ ! -d "$D" ]; then
            echo "oat-run: no exec $id" >&2
            exit 98
        fi
        ;;
    *)
        echo "oat-run: unknown mode $mode" >&2
        exit 96
        ;;
esac

# Everything the command has written that has not been passed on yet. Reading
# the files ourselves rather than following them with `tail -f` is what makes
# the last line as reliable as the first: a follower polls on its own schedule
# and loses whatever arrives between its final look and being stopped.
out_pos=0
err_pos=0
while :; do
    out_size=$(wc -c < "$D/out" 2>/dev/null || echo 0)
    if [ "$out_size" -gt "$out_pos" ]; then
        tail -c +$((out_pos + 1)) "$D/out" | head -c $((out_size - out_pos))
        out_pos=$out_size
    fi
    err_size=$(wc -c < "$D/err" 2>/dev/null || echo 0)
    if [ "$err_size" -gt "$err_pos" ]; then
        tail -c +$((err_pos + 1)) "$D/err" | head -c $((err_size - err_pos)) >&2
        err_pos=$err_size
    fi

    if [ -f "$D/exit" ]; then
        # Stop only once the files hold nothing newer than what was passed on.
        if [ "$out_pos" -eq "$(wc -c < "$D/out" 2>/dev/null || echo 0)" ] &&
            [ "$err_pos" -eq "$(wc -c < "$D/err" 2>/dev/null || echo 0)" ]; then
            break
        fi
    elif [ -f "$D/pid" ] && ! kill -0 "$(cat "$D/pid")" 2>/dev/null; then
        sleep 0.5
        if [ ! -f "$D/exit" ]; then
            echo "oat-run: exec $id vanished without an exit code" >&2
            exit 99
        fi
    fi
    sleep 0.2
done

exit "$(cat "$D/exit")"
