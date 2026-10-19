# Kelta shell integration (#526). Kelta put its folder first in XDG_DATA_DIRS so fish reads this
# file: take it back out, then report the folder and command marks from interactive shells.
if set -q KELTA_FISH_XDG_DATA_DIR; and set -q XDG_DATA_DIRS
    set --global --export --path XDG_DATA_DIRS "$XDG_DATA_DIRS"
    if set -l i (contains -i -- "$KELTA_FISH_XDG_DATA_DIR" $XDG_DATA_DIRS)
        set --erase --global XDG_DATA_DIRS[$i]
        test -n "$XDG_DATA_DIRS"; or set --erase --global XDG_DATA_DIRS
    end
    set -q XDG_DATA_DIRS; and set --global --export --unpath XDG_DATA_DIRS "$XDG_DATA_DIRS"
end
set --erase KELTA_FISH_XDG_DATA_DIR
status is-interactive; or exit

function __kelta_prompt --on-event fish_prompt
    printf '\e]7;file://%s%s\a\e]133;A\a' $hostname (string escape --style=url -- $PWD)
end
function __kelta_preexec --on-event fish_preexec
    printf '\e]133;C\a'
end
function __kelta_postexec --on-event fish_postexec
    printf '\e]133;D;%s\a' $status
end
