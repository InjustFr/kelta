// Icon names available in src/assets/sprite.svg (Lucide subset, ISC). Symbols have id `i-<name>`.

import spriteSvg from '../../assets/sprite.svg?raw';

export const ICON_NAMES = [
  'x',
  'plus',
  'minus',
  'check',
  'chevron-down',
  'chevron-up',
  'chevron-left',
  'chevron-right',
  'search',
  'settings',
  'square-terminal',
  'terminal',
  'folder',
  'folder-open',
  'folder-plus',
  'git-branch',
  'git-pull-request',
  'git-merge',
  'git-commit-horizontal',
  'inbox',
  'ticket',
  'kanban',
  'list',
  'columns-2',
  'rows-2',
  'maximize-2',
  'minimize-2',
  'panel-left',
  'ellipsis',
  'ellipsis-vertical',
  'external-link',
  'refresh-cw',
  'play',
  'square',
  'circle-alert',
  'triangle-alert',
  'info',
  'circle-check',
  'circle-x',
  'circle',
  'bell',
  'bell-off',
  'bot',
  'file-code',
  'file-text',
  'pencil',
  'trash-2',
  'copy',
  'clipboard',
  'link',
  'eye',
  'eye-off',
  'lock',
  'lock-open',
  'key-round',
  'user',
  'users',
  'message-square',
  'send',
  'plug',
  'puzzle',
  'wrench',
  'zap',
  'house',
  'globe',
  'cpu',
  'activity',
  'loader-circle',
  'keyboard',
  'command',
  'arrow-right',
  'arrow-left',
  'arrow-up',
  'arrow-down',
  'filter',
  'sun',
  'moon',
  'monitor',
  'app-window',
  'gauge',
  'star',
  'tag',
  'clock',
  'history',
  'box',
  'database',
  'grip-vertical',
  'layers',
  'shield-check',
] as const;

export type IconName = (typeof ICON_NAMES)[number];

export function isIconName(name: string): name is IconName {
  return (ICON_NAMES as readonly string[]).includes(name);
}

const SPRITE_ID = 'kelta-sprite';

/** Inlines the sprite once into <body> so `<use href="#i-name">` works under every scheme. */
export function injectSprite(doc: Document = document): void {
  if (doc.getElementById(SPRITE_ID)) return;
  const holder = doc.createElement('div');
  holder.id = SPRITE_ID;
  holder.setAttribute('aria-hidden', 'true');
  holder.style.display = 'none';
  holder.innerHTML = spriteSvg;
  doc.body.prepend(holder);
}

export { spriteSvg };
