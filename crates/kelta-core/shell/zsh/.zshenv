# Kelta shell integration (#526). Kelta points ZDOTDIR here only so zsh reads this file first:
# put the user's ZDOTDIR back, read their .zshenv, then add the prompt hooks to interactive shells.
if [[ -n "${KELTA_ZSH_ZDOTDIR+X}" ]]; then
  'builtin' 'export' ZDOTDIR="$KELTA_ZSH_ZDOTDIR"
  'builtin' 'unset' 'KELTA_ZSH_ZDOTDIR'
else
  'builtin' 'unset' 'ZDOTDIR'
fi
{
  'builtin' 'typeset' _kelta_file=${ZDOTDIR-$HOME}"/.zshenv"
  [[ ! -r "$_kelta_file" ]] || 'builtin' 'source' '--' "$_kelta_file"
} always {
  'builtin' 'unset' '_kelta_file'
  if [[ -o interactive ]] && (( ! $+functions[_kelta_precmd] )); then
    'builtin' 'typeset' -gi _kelta_fd=1 _kelta_ran=0
    # p10k instant prompt redirects stdout while .zshrc runs: write to the terminal itself.
    { 'builtin' 'zmodload' zsh/system && [[ -w $TTY ]] && 'builtin' 'sysopen' -o cloexec -wu _kelta_fd -- $TTY } 2>/dev/null
    _kelta_precmd() {
      'builtin' 'local' ret=$? url= c
      'builtin' 'emulate' -L zsh -o no_multibyte
      (( _kelta_ran )) && 'builtin' 'print' -nu $_kelta_fd "\e]133;D;$ret\a"
      _kelta_ran=0
      for c in ${(s::)PWD}; do
        if [[ $c == [a-zA-Z0-9/._~-] ]]; then url+=$c; else 'builtin' 'printf' -v c '%%%02X' "'$c"; url+=$c; fi
      done
      'builtin' 'print' -nu $_kelta_fd "\e]7;file://$HOST$url\a\e]133;A\a"
    }
    _kelta_preexec() {
      _kelta_ran=1
      'builtin' 'print' -nu $_kelta_fd '\e]133;C\a'
    }
    'builtin' 'typeset' -ag precmd_functions preexec_functions
    precmd_functions=(_kelta_precmd $precmd_functions)
    preexec_functions+=(_kelta_preexec)
  fi
}
