// Query-swallowing parser handlers (ARCHITECTURE §7.4). The Rust model answers terminal queries
// (DA1/DA2/DSR/DECRQM, OSC 4/10/11/12 `?`); xterm.js must not answer them a second time, so these
// handlers swallow exactly the list in `gen/terminal_queries.ts` (generated from
// `kelta_proto::term::SWALLOWED_QUERIES`) by returning `true`. Queries the model does not answer
// (e.g. XTVERSION) are not registered and keep xterm's own behaviour.

import { SWALLOWED_CSI, SWALLOWED_OSC, type CsiQuery, type OscQuery } from '$lib/gen/terminal_queries';

interface Disposable {
  dispose(): void;
}

/** The slice of `term.parser` used here (xterm and @xterm/headless both satisfy it). */
export interface ParserLike {
  registerCsiHandler(
    id: { prefix?: string; intermediates?: string; final: string },
    callback: (params: (number | number[])[]) => boolean | Promise<boolean>,
  ): Disposable;
  registerOscHandler(ident: number, callback: (data: string) => boolean | Promise<boolean>): Disposable;
}

function firstParam(params: (number | number[])[]): number | undefined {
  const p = params[0];
  return Array.isArray(p) ? p[0] : p;
}

/** Whether an OSC payload is a query (`?`) rather than a colour change. */
export function isOscQuery(query: OscQuery, data: string): boolean {
  if (!query.queryOnly) return true;
  const parts = data.split(';');
  if (query.ident === 4) {
    // OSC 4 ; index ; spec [; index ; spec]… — a query when some spec is `?`.
    return parts.some((part, i) => i % 2 === 1 && part === '?');
  }
  return parts.length > 0 && parts.every((part) => part === '?');
}

export function csiMatches(query: CsiQuery, params: (number | number[])[]): boolean {
  if (query.params.length === 0) return true;
  const first = firstParam(params);
  return first !== undefined && query.params.includes(first);
}

export function installQueryHandlers(
  parser: ParserLike,
  csi: readonly CsiQuery[] = SWALLOWED_CSI,
  osc: readonly OscQuery[] = SWALLOWED_OSC,
): Disposable[] {
  const out: Disposable[] = [];
  for (const query of csi) {
    out.push(
      parser.registerCsiHandler(
        { prefix: query.prefix, intermediates: query.intermediates, final: query.final },
        (params) => csiMatches(query, params),
      ),
    );
  }
  for (const query of osc) {
    out.push(parser.registerOscHandler(query.ident, (data) => isOscQuery(query, data)));
  }
  return out;
}
