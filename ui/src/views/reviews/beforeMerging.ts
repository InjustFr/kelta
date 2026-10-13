// "Before merging" (#401): the description's section on deploy steps, migrations and transitional code,
// lifted out so it is not missed. Its source is the PR template: a heading (or a line in bold alone)
// that names it, up to the next heading.

const TITLE =
  /^(before merg|after merg|deploy|deployment|migrations?|rollout|release|post[- ]?(deploy|merge)|avant (de )?(merger|fusionner|déployer)|déploiement|vor dem (merge|deploy))/i;

const isHeading = (el: Element): boolean =>
  /^H[1-6]$/.test(el.tagName) ||
  (el.tagName === 'P' &&
    el.children.length === 1 &&
    /^(STRONG|B)$/.test(el.children[0]!.tagName) &&
    el.textContent.trim() === el.children[0]!.textContent.trim());

/** `body` (sanitized HTML) split into the before-merging section (`null`: none) and the rest. */
export function beforeMerging(body: string): { before: string | null; rest: string } {
  const doc = new DOMParser().parseFromString(body, 'text/html');
  const start = [...doc.body.children].find((el) => isHeading(el) && TITLE.test(el.textContent.trim()));
  if (!start) return { before: null, rest: body };
  const taken: Element[] = [];
  for (let el = start.nextElementSibling; el && !isHeading(el); el = el.nextElementSibling) taken.push(el);
  const before = taken.map((el) => el.outerHTML).join('');
  if (before.trim() === '') return { before: null, rest: body };
  start.remove();
  taken.forEach((el) => el.remove());
  return { before, rest: doc.body.innerHTML };
}
