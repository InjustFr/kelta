// The ticket's description read as a spec (#400): one foldable section per heading, the section that
// says what is still open pulled out, raw ticket links turned into labelled chips, and acceptance
// criteria written Given / When / Then laid out as such. Works on HTML already sanitized in Rust:
// it only moves that HTML around and adds text through `textContent`.

export interface SpecSection {
  /** The heading's text; '' for what comes before the first heading. */
  title: string;
  html: string;
  /** Items of a section that is only a list (References 5). */
  count: number | null;
}

export interface Spec {
  sections: SpecSection[];
  /** What the spec still leaves open: a section headed Open questions, Missing, To clarify… */
  open: SpecSection | null;
}

/** A ticket a link points to: its key and title. */
export type RefLabel = (href: string) => { key: string; title: string } | null;

/** Heading names that mark what is still open. shortcut: a fixed list, a Claude summary could replace it. */
const OPEN =
  /^(open questions?|questions?|missing|still open|to clarify|unknowns?|tbd|questions ouvertes|à préciser|manquant|offene fragen|fehlt)\b/i;
const HEADING = /^H[1-6]$/;
const KEYWORD = /^\s*(?:<(strong|b|em)>\s*)?(given|when|then|and|but)\b\s*:?\s*(?:<\/\1>)?\s*:?\s*/i;

export function readSpec(html: string, label: RefLabel = () => null): Spec {
  const tpl = document.createElement('template');
  tpl.innerHTML = html;
  let box: ParentNode = tpl.content;
  // Some trackers wrap the whole body in one block.
  while (box.children.length === 1 && /^(DIV|SECTION|ARTICLE)$/.test(box.firstElementChild!.tagName))
    box = box.firstElementChild!;

  const parts: { title: string; body: HTMLElement }[] = [];
  let body: HTMLElement | null = null;
  for (const node of [...box.childNodes]) {
    if (node instanceof HTMLElement && HEADING.test(node.tagName)) {
      body = document.createElement('div');
      parts.push({ title: node.textContent?.trim() ?? '', body });
    } else {
      if (!body) {
        if (!node.textContent?.trim() && !(node instanceof HTMLElement)) continue;
        body = document.createElement('div');
        parts.push({ title: '', body });
      }
      body.append(node);
    }
  }

  const sections = parts
    .map(({ title, body }) => {
      chips(body, label);
      gwt(body);
      const blocks = [...body.children].filter((b) => b.tagName !== 'HR');
      const lists = blocks.length > 0 && blocks.every((b) => b.tagName === 'UL' || b.tagName === 'OL');
      return {
        title,
        html: body.innerHTML.trim(),
        count: lists ? blocks.reduce((n, b) => n + b.querySelectorAll(':scope > li').length, 0) : null,
      };
    })
    .filter((s) => s.html !== '' || s.title !== '');
  const open = sections.find((s) => s.title && OPEN.test(s.title)) ?? null;
  return { sections: sections.filter((s) => s !== open), open };
}

/** A key read off a ticket URL the list does not have: Jira `/browse/KEY-1`, `/issues/12`, `/pull/12`. */
function keyOf(href: string): string | null {
  const m = /\/browse\/([A-Z][A-Z0-9_]*-\d+)|\/(?:issues|pull|merge_requests)\/(\d+)(?:[/?#]|$)/.exec(href);
  return m ? (m[1] ?? `#${m[2]}`) : null;
}

/** Links shown as their raw URL become a chip: the ticket's key, then its title when known. */
function chips(body: HTMLElement, label: RefLabel): void {
  for (const a of body.querySelectorAll('a[href]')) {
    const href = a.getAttribute('href') ?? '';
    const text = a.textContent?.trim() ?? '';
    if (text !== href && !/^https?:\/\//.test(text)) continue;
    const known = label(href);
    const key = known?.key ?? keyOf(href);
    if (!key) continue;
    const k = document.createElement('span');
    k.className = 'ref-key';
    k.textContent = key;
    a.replaceChildren(k, ...(known?.title ? [known.title] : []));
    a.classList.add('ref');
  }
}

interface Line {
  word: string;
  html: string;
}

function line(html: string): Line | null {
  const m = KEYWORD.exec(html);
  return m ? { word: m[2]!, html: html.slice(m[0].length) } : null;
}

/** The criterion as a definition list: Given / When / Then on the left, the rest on the right. */
function list(lines: Line[]): HTMLElement {
  const dl = document.createElement('dl');
  dl.className = 'gwt';
  for (const l of lines) {
    const dt = document.createElement('dt');
    dt.textContent = l.word[0]!.toUpperCase() + l.word.slice(1).toLowerCase();
    const dd = document.createElement('dd');
    dd.innerHTML = l.html;
    dl.append(dt, dd);
  }
  return dl;
}

const complete = (lines: Line[]): boolean =>
  lines.some((l) => /^given$/i.test(l.word)) && lines.some((l) => /^(when|then)$/i.test(l.word));

function gwt(body: HTMLElement): void {
  // One block holding the whole criterion, a line each (`<br>` or newlines): the lines before Given title it.
  for (const el of body.querySelectorAll('p, li')) {
    if (el.querySelector('p, li, ul, ol')) continue;
    const raw = el.innerHTML
      .split(/<br\s*\/?>|\n/i)
      .map((s) => s.trim())
      .filter(Boolean);
    const first = raw.findIndex((s) => line(s));
    if (first < 0) continue;
    const lines: Line[] = [];
    for (const s of raw.slice(first)) {
      const l = line(s);
      if (l) lines.push(l);
      else lines[lines.length - 1]!.html += ` ${s}`;
    }
    if (!complete(lines)) continue;
    const title = raw.slice(0, first).join(' ');
    // A `<dl>` cannot sit in a `<p>` once the HTML is parsed again: a paragraph becomes a block.
    const box = el.tagName === 'P' ? document.createElement('div') : el;
    box.replaceChildren();
    if (title) {
      const strong = document.createElement('strong');
      strong.className = 'gwt-title';
      strong.innerHTML = title;
      box.append(strong);
    }
    box.append(list(lines));
    if (box !== el) el.replaceWith(box);
  }
  // Or one paragraph per keyword: the run becomes one list in place of its first paragraph.
  let run: { el: Element; line: Line }[] = [];
  const flush = (): void => {
    const lines = run.map((r) => r.line);
    if (complete(lines)) {
      run[0]!.el.replaceWith(list(lines));
      for (const r of run.slice(1)) r.el.remove();
    }
    run = [];
  };
  for (const el of [...body.children]) {
    const l = el.tagName === 'P' ? line(el.innerHTML.trim()) : null;
    if (l) run.push({ el, line: l });
    else flush();
  }
  flush();
}

export interface Criterion {
  text: string;
  checked: boolean;
}

export interface Criteria {
  items: Criterion[];
  /** The description's first paragraph, for a ticket without criteria. */
  intro: string;
}

/** Heading names of the acceptance criteria. */
const ACCEPTANCE = /^(acceptance|acceptance criteria|ac)\s*:?$/i;

/**
 * The acceptance criteria of a description: its task-list items, else the list under a heading named
 * Acceptance, Acceptance criteria or AC. shortcut: English headings only, like the Open questions list.
 */
export function readCriteria(html: string): Criteria {
  const tpl = document.createElement('template');
  tpl.innerHTML = html;
  const root = tpl.content;
  const text = (li: Element): string => {
    const c = li.cloneNode(true) as Element;
    c.querySelectorAll('input, ul, ol').forEach((n) => n.remove());
    return c.textContent?.replace(/\s+/g, ' ').trim() ?? '';
  };
  const item = (li: Element): Criterion => ({
    text: text(li),
    checked: li.querySelector(':scope > input[checked], :scope > p > input[checked]') !== null,
  });

  let lis: Element[] = [...root.querySelectorAll('li')].filter((li) =>
    li.querySelector(':scope > input[type=checkbox], :scope > p > input[type=checkbox]'),
  );
  if (lis.length === 0) {
    const h = [...root.querySelectorAll('h1, h2, h3, h4, h5, h6')].find((e) =>
      ACCEPTANCE.test(e.textContent?.trim() ?? ''),
    );
    for (let el = h?.nextElementSibling; el && !HEADING.test(el.tagName); el = el.nextElementSibling)
      if (/^[UO]L$/.test(el.tagName)) lis.push(...el.querySelectorAll(':scope > li'));
  }
  lis = lis.filter((li) => text(li) !== '');

  const first = root.querySelector('p, pre')?.textContent ?? '';
  return {
    items: lis.map(item),
    intro: first
      .trim()
      .split(/\n\s*\n/)[0]!
      .trim(),
  };
}
