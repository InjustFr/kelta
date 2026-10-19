# Kelta shell integration (#526). Kelta starts bash as `bash --posix` with ENV pointing here, so bash
# reads this file instead of its startup files: leave POSIX mode, read them, add the prompt hooks.
if [[ -n "$KELTA_BASH_INJECT" ]]; then
  builtin declare __kelta_inject="$KELTA_BASH_INJECT"
  builtin unset ENV KELTA_BASH_INJECT
  builtin set +o posix
  if [[ -n "$KELTA_BASH_UNEXPORT_HISTFILE" ]]; then
    builtin export -n HISTFILE
    builtin unset KELTA_BASH_UNEXPORT_HISTFILE
  fi
  if [[ "$__kelta_inject" == *--login* ]]; then
    [[ -r /etc/profile ]] && builtin source /etc/profile
    for __kelta_f in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
      [[ -r "$__kelta_f" ]] && { builtin source "$__kelta_f"; break; }
    done
  else
    [[ -r /etc/bash.bashrc ]] && builtin source /etc/bash.bashrc
    [[ -r "$HOME/.bashrc" ]] && builtin source "$HOME/.bashrc"
  fi
  builtin unset __kelta_f __kelta_inject
fi

__kelta_ran=
__kelta_precmd() {
  builtin local ret=$? LC_ALL=C url= c i
  if [[ -n "$__kelta_ran" ]]; then builtin printf '\e]133;D;%s\a' "$ret"; __kelta_ran=; fi
  for ((i = 0; i < ${#PWD}; i++)); do
    c=${PWD:i:1}
    case $c in
      [a-zA-Z0-9/._~-]) url+=$c ;;
      *) builtin printf -v c '%%%02X' "'$c"; url+=$c ;;
    esac
  done
  builtin printf '\e]7;file://%s%s\a\e]133;A\a' "$HOSTNAME" "$url"
  builtin return "$ret"
}
# PS0 prints before each command; the zero-length substring only sets the "a command ran" flag.
PS0='\e]133;C\a${__kelta_ran:$((__kelta_ran=1)):0}'"${PS0-}"
PROMPT_COMMAND="__kelta_precmd${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
