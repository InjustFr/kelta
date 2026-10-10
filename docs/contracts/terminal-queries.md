# Terminal queries answered by the Rust model (L1)

ARCHITECTURE §7.4: the headless `alacritty_terminal` model of each session (`kelta-term`) is the
single responder to terminal queries, so background sessions answer correctly and nothing is
answered twice. xterm.js swallows exactly the queries the model answers
(`kelta_proto::term::SWALLOWED_QUERIES` → `ui/src/lib/gen/terminal_queries.ts`).

The table below is the contract **and** test data: `crates/kelta-term/tests/it/queries.rs` parses
the rows between the markers, feeds every query to a fresh 80×24 model (cursor at 1;1, default
`TerminalPalette`, no output yet) and asserts the exact reply; it then sends all of them through
a real PTY and asserts that the child reads back each reply exactly once, in order. It also
checks that the names marked `yes` in the last column are exactly the `SWALLOWED_QUERIES` names.

Escapes: `\e` = ESC, `\a` = BEL, `\\` = backslash. `—` = no reply.

<!-- queries:begin -->
| Name | Query | Reply | Swallowed |
|---|---|---|---|
| DA1 | `\e[c` | `\e[?6c` | yes |
| DA1 | `\e[0c` | `\e[?6c` | yes |
| DA2 | `\e[>c` | `\e[>0;2600;1c` | yes |
| DA2 | `\e[>0c` | `\e[>0;2600;1c` | yes |
| DSR | `\e[5n` | `\e[0n` | yes |
| DSR | `\e[6n` | `\e[1;1R` | yes |
| DECRQM | `\e[?25$p` | `\e[?25;1$y` | yes |
| DECRQM | `\e[?2004$p` | `\e[?2004;2$y` | yes |
| DECRQM | `\e[?2026$p` | `\e[?2026;2$y` | yes |
| DECRQM | `\e[?1049$p` | `\e[?1049;2$y` | yes |
| DECRQM | `\e[?9999$p` | `\e[?9999;0$y` | yes |
| DECRQM_ANSI | `\e[4$p` | `\e[4;2$y` | yes |
| DECRQM_ANSI | `\e[99$p` | `\e[99;0$y` | yes |
| OSC4_PALETTE | `\e]4;1;?\a` | `\e]4;1;rgb:cdcd/3131/3131\a` | yes |
| OSC4_PALETTE | `\e]4;196;?\e\\` | `\e]4;196;rgb:ffff/0000/0000\e\\` | yes |
| OSC10_FG | `\e]10;?\a` | `\e]10;rgb:d4d4/d4d4/d4d4\a` | yes |
| OSC11_BG | `\e]11;?\a` | `\e]11;rgb:1e1e/1e1e/1e1e\a` | yes |
| OSC11_BG | `\e]11;?\e\\` | `\e]11;rgb:1e1e/1e1e/1e1e\e\\` | yes |
| OSC12_CURSOR | `\e]12;?\a` | `\e]12;rgb:d4d4/d4d4/d4d4\a` | yes |
| XTWINOPS_CHARS | `\e[18t` | `\e[8;24;80t` | yes |
| KITTY_KEYBOARD | `\e[?u` | `\e[?0u` | yes |
| DA3 | `\e[=c` | — | no |
| DECXCPR | `\e[?6n` | — | no |
| XTVERSION | `\e[>0q` | — | no |
| XTWINOPS_PIXELS | `\e[14t` | — | no |
| XTQMODKEYS | `\e[?4m` | — | no |
| OSC52_READ | `\e]52;c;?\a` | — | no |
| DECRQSS | `\eP$qm\e\\` | — | no |
<!-- queries:end -->

## Notes

- OSC 4/10/11/12 replies come from the palette pushed with `terminal_set_palette` (re-pushed on
  theme change); a colour the application set itself (`OSC 11;#rrggbb`) wins until it is reset
  (`OSC 111`). The reply uses the query's terminator (BEL or ST).
- DA2 reports alacritty_terminal's version (`0.26.0` → `2600`).
- DECRQM answers every mode: `1` set, `2` reset, `0` unknown. `?2026` always reports `2`
  (supported, not active): Kelta flushes synchronized updates itself.
- Queries inside a DEC 2026 synchronized update are answered when the update ends (at the
  latest after alacritty's 150 ms deadline).
- Kitty keyboard (`terminal.keyboard_protocol = "kitty"`, the default): the model tracks the
  `CSI > u` / `CSI < u` / `CSI = u` mode stacks and answers `CSI ? u` with the active stack top.
  With `"legacy"` it is not answered (still swallowed: xterm.js 6.0 has no kitty support), so
  applications fall back via DA1.
- OSC 52 is copy-only: reads are not answered (and not swallowed).
- XTWINOPS 18 (`CSI 18 t`, text area size in characters) is answered by the model and swallowed,
  so the swallowed list stays the exact answered set even if xterm.js `windowOptions` is enabled.
- Rows marked `—` are not answered by the model and must not be swallowed. xterm.js answers some
  of them itself (for example DECRQSS and DECXCPR) while a view is attached; hidden sessions
  leave them unanswered, as most terminals do for unknown queries.
