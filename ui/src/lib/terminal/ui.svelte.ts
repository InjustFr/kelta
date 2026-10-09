// Reactive terminal UI state shared between actions and panes (search bar target).

class TerminalUi {
  /** Session whose search bar is open (`terminal.search`). */
  searchSession = $state<string | null>(null);
}

export const terminalUi = new TerminalUi();
