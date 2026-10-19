// Icons are Lucide (https://lucide.dev, `lucide-static`, ISC License, Copyright (c) Lucide Icons and Contributors).
// The names the UI uses are inlined in one sprite at startup; any other Lucide name (a config `icon`) loads on
// first use. Symbols have id `i-<name>`.

import * as lucide from 'lucide-static';
import type LUCIDE_TAGS from 'lucide-static/tags.json';
import LUCIDE_NAMES from 'virtual:lucide-names';

export type IconName = keyof typeof LUCIDE_TAGS;

/** Every Lucide name, aliases included (`IconName` lists the current names only). */
export const ICON_NAMES = LUCIDE_NAMES as IconName[];

const NAME_SET: ReadonlySet<string> = new Set(ICON_NAMES);

export function isIconName(name: string): name is IconName {
  return NAME_SET.has(name);
}

/** Built into the startup sprite (tree-shaken from the package). */
const SPRITE_ICONS = {
  x: lucide.X,
  plus: lucide.Plus,
  minus: lucide.Minus,
  check: lucide.Check,
  'chevron-down': lucide.ChevronDown,
  'chevron-up': lucide.ChevronUp,
  'chevron-left': lucide.ChevronLeft,
  'chevron-right': lucide.ChevronRight,
  search: lucide.Search,
  settings: lucide.Settings,
  'square-terminal': lucide.SquareTerminal,
  terminal: lucide.Terminal,
  folder: lucide.Folder,
  'folder-open': lucide.FolderOpen,
  'folder-plus': lucide.FolderPlus,
  'git-branch': lucide.GitBranch,
  'git-pull-request': lucide.GitPullRequest,
  'git-merge': lucide.GitMerge,
  'git-commit-horizontal': lucide.GitCommitHorizontal,
  inbox: lucide.Inbox,
  ticket: lucide.Ticket,
  kanban: lucide.Kanban,
  list: lucide.List,
  'columns-2': lucide.Columns2,
  'rows-2': lucide.Rows2,
  'maximize-2': lucide.Maximize2,
  'minimize-2': lucide.Minimize2,
  'panel-left': lucide.PanelLeft,
  ellipsis: lucide.Ellipsis,
  'ellipsis-vertical': lucide.EllipsisVertical,
  'external-link': lucide.ExternalLink,
  'refresh-cw': lucide.RefreshCw,
  play: lucide.Play,
  square: lucide.Square,
  'circle-alert': lucide.CircleAlert,
  'triangle-alert': lucide.TriangleAlert,
  info: lucide.Info,
  'circle-check': lucide.CircleCheck,
  'circle-x': lucide.CircleX,
  circle: lucide.Circle,
  bell: lucide.Bell,
  'bell-off': lucide.BellOff,
  bot: lucide.Bot,
  'file-code': lucide.FileCode,
  'file-text': lucide.FileText,
  pencil: lucide.Pencil,
  trash: lucide.Trash,
  copy: lucide.Copy,
  clipboard: lucide.Clipboard,
  link: lucide.Link,
  eye: lucide.Eye,
  'eye-off': lucide.EyeOff,
  lock: lucide.Lock,
  'lock-open': lucide.LockOpen,
  'key-round': lucide.KeyRound,
  user: lucide.User,
  users: lucide.Users,
  'message-square': lucide.MessageSquare,
  send: lucide.Send,
  plug: lucide.Plug,
  puzzle: lucide.Puzzle,
  wrench: lucide.Wrench,
  zap: lucide.Zap,
  house: lucide.House,
  globe: lucide.Globe,
  cpu: lucide.Cpu,
  activity: lucide.Activity,
  'loader-circle': lucide.LoaderCircle,
  keyboard: lucide.Keyboard,
  command: lucide.Command,
  'arrow-right': lucide.ArrowRight,
  'arrow-left': lucide.ArrowLeft,
  'arrow-up': lucide.ArrowUp,
  'arrow-down': lucide.ArrowDown,
  funnel: lucide.Funnel,
  sun: lucide.Sun,
  moon: lucide.Moon,
  monitor: lucide.Monitor,
  'app-window': lucide.AppWindow,
  gauge: lucide.Gauge,
  star: lucide.Star,
  tag: lucide.Tag,
  clock: lucide.Clock,
  'rotate-ccw-clock': lucide.RotateCcwClock,
  box: lucide.Box,
  database: lucide.Database,
  'grip-vertical': lucide.GripVertical,
  layers: lucide.Layers,
  'shield-check': lucide.ShieldCheck,
  'list-tree': lucide.ListTree,
  container: lucide.Container,
} satisfies Partial<Record<IconName, string>>;

/** A Lucide SVG string as a sprite symbol (viewBox and children only, as `Icon` styles the stroke). */
function toSymbol(name: string, svg: string): string {
  const body = /<svg[^>]*>([\s\S]*)<\/svg>/.exec(svg)?.[1] ?? '';
  return `<symbol id="i-${name}" viewBox="0 0 24 24">${body.trim()}</symbol>`;
}

const symbols = Object.entries(SPRITE_ICONS).map(([name, svg]) => toSymbol(name, svg));
export const spriteSvg = `<svg xmlns="http://www.w3.org/2000/svg" style="display:none"><!-- Lucide (https://lucide.dev), ISC License, Copyright (c) Lucide Icons and Contributors -->${symbols.join('')}</svg>`;

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

const loading = new Map<string, Promise<void>>();

/**
 * Adds a Lucide icon outside the startup sprite to it. Undefined when there is nothing to load (no sprite, already
 * in it, or not a Lucide name), else resolves once the symbol is in.
 */
export function ensureIcon(name: string, doc: Document = document): Promise<void> | undefined {
  const sprite = doc.querySelector(`#${SPRITE_ID} > svg`);
  if (!sprite || !isIconName(name) || doc.getElementById(`i-${name}`)) return undefined;
  let pending = loading.get(name);
  if (!pending) {
    pending = import('./lucide')
      .then(async ({ svgOf }) => {
        const svg = await svgOf(name);
        if (!doc.getElementById(`i-${name}`)) sprite.insertAdjacentHTML('beforeend', toSymbol(name, svg));
      })
      // A chunk that failed to load leaves the icon blank; the next use tries again.
      .catch((error: unknown) => {
        console.warn(`icon ${name} failed to load`, error);
        loading.delete(name);
      });
    loading.set(name, pending);
  }
  return pending;
}
