#!/usr/bin/env node
// Bundle budgets (ARCHITECTURE §13): initial JS <= 200 KB gz, each lazy chunk <= 120 KB gz,
// optional installers (dmg <= 20 MB, deb <= 15 MB). Usage: node bench/bundle-size.mjs [ui/dist] [artifact...]
import { readFileSync, readdirSync, statSync } from "node:fs";
import { gzipSync } from "node:zlib";
import { join, extname } from "node:path";
import { pathToFileURL } from "node:url";

const KB = 1024;
export const LIMITS = { initialJs: 200 * KB, lazyChunk: 120 * KB, dmg: 20 * KB * KB, deb: 15 * KB * KB };

/** JS files the HTML loads eagerly (script src, modulepreload). */
export function initialFiles(html) {
  const re = /<(?:script[^>]*\ssrc|link[^>]*rel="modulepreload"[^>]*\shref)="\/?([^"]+\.js)"/g;
  return new Set([...html.matchAll(re)].map((m) => m[1]));
}

function walk(dir, base = dir) {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? walk(p, base) : [p.slice(base.length + 1)];
  });
}

export function check(dist, artifacts = []) {
  const gz = (f) => gzipSync(readFileSync(join(dist, f))).length;
  const eager = initialFiles(readFileSync(join(dist, "index.html"), "utf8"));
  const js = walk(dist).filter((f) => extname(f) === ".js");
  const initial = [...eager].reduce((n, f) => n + gz(f), 0);
  const rows = [{ name: "initial JS (gz)", size: initial, limit: LIMITS.initialJs }];
  for (const f of js.filter((f) => !eager.has(f))) rows.push({ name: `lazy ${f} (gz)`, size: gz(f), limit: LIMITS.lazyChunk });
  for (const a of artifacts) {
    const kind = a.endsWith(".dmg") ? "dmg" : a.endsWith(".deb") ? "deb" : null;
    if (kind) rows.push({ name: a, size: statSync(a).size, limit: LIMITS[kind] });
  }
  return rows.map((r) => ({ ...r, ok: r.size <= r.limit }));
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [dist = "ui/dist", ...artifacts] = process.argv.slice(2);
  const rows = check(dist, artifacts);
  for (const r of rows) console.log(`${r.ok ? "ok  " : "FAIL"} ${r.name}: ${(r.size / KB).toFixed(1)} KB (limit ${(r.limit / KB).toFixed(0)} KB)`);
  process.exit(rows.every((r) => r.ok) ? 0 : 1);
}
