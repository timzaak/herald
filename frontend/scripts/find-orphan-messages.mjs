#!/usr/bin/env node
/**
 * Detect orphan i18n message keys.
 *
 * Herald uses @inlang/paraglide-js. Messages are consumed as
 * `m['namespace.key']()` (bracket-notation with the dotted string key).
 *
 * This script:
 *   1. Collects all dotted message keys from messages/en.json (the source locale).
 *   2. Scans src/ (excluding the paraglide compiled output and test files) for
 *      `m['<key>']` or `m["<key>"]` references.
 *   3. Reports keys with zero references as orphans.
 *
 * Exit code 1 if orphans are found (suitable for CI), 0 otherwise.
 * Pass --verbose to also print used-key stats.
 */
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, extname, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

// URL.pathname yields "/C:/..." on Windows, which Node resolves against the
// current drive root (producing "C:\C:\..."). fileURLToPath returns a native
// absolute path on every platform.
const FRONTEND_ROOT = fileURLToPath(new URL('../', import.meta.url))
const SOURCE_LOCALE = JSON.parse(
  readFileSync(new URL('../messages/en.json', import.meta.url), 'utf8')
)
const SRC_ROOT = fileURLToPath(new URL('../src/', import.meta.url))
const SKIP_DIRS = new Set(['paraglide', '__tests__', 'node_modules'])
const SCAN_EXTS = new Set(['.tsx', '.ts', '.jsx', '.js'])
const verbose = process.argv.includes('--verbose')

// 1. Flatten nested message JSON into dotted keys.
function collectKeys(obj, prefix = '', out = []) {
  for (const [k, v] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${k}` : k
    if (v !== null && typeof v === 'object' && !Array.isArray(v)) {
      collectKeys(v, path, out)
    } else {
      out.push(path)
    }
  }
  return out
}

const allKeys = collectKeys(SOURCE_LOCALE)

// 2. Walk src/ and collect all source text (excluding skipped dirs).
const fileTexts = []
function walk(dir) {
  for (const entry of readdirSync(dir)) {
    if (SKIP_DIRS.has(entry)) continue
    const full = join(dir, entry)
    const st = statSync(full)
    if (st.isDirectory()) {
      walk(full)
    } else if (SCAN_EXTS.has(extname(full)) && !entry.includes('.test.')) {
      fileTexts.push({ path: relative(FRONTEND_ROOT, full), text: readFileSync(full, 'utf8') })
    }
  }
}
walk(SRC_ROOT)

// 3. Collect dynamic reference evidence:
//    A key appearing as a quoted string literal anywhere in src/ counts as
//    referenced even outside an `m[...]` call — records that map enum values
//    to message keys and later call `m[record[x]]()` (e.g.
//    src/types/billing.ts, currency-purchase-group.tsx) are legitimate
//    consumers the static `m['key']` regex cannot see.
const templatePrefixes = []
for (const { text } of fileTexts) {
  // `m[`points.balance_type_${k}`]` — any key starting with the captured
  // literal prefix is constructible at runtime.
  for (const match of text.matchAll(/m\[`([a-z0-9_.]+)\$\{/g)) {
    templatePrefixes.push(match[1])
  }
}
const templatePrefixSet = new Set(templatePrefixes)

// 4. For each key, check if it is referenced statically or dynamically.
const orphans = []
let usedCount = 0
for (const key of allKeys) {
  // Match m['common.save'] or m["common.save"] — escape the key for regex.
  const escaped = key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const staticRe = new RegExp(`m\\[['"]${escaped}['"]\\]`)
  const literalRe = new RegExp(`['"]${escaped}['"]`)
  const found =
    fileTexts.some((f) => staticRe.test(f.text)) ||
    fileTexts.some((f) => literalRe.test(f.text)) ||
    [...templatePrefixSet].some((p) => key.startsWith(p))
  if (found) {
    usedCount++
  } else {
    orphans.push(key)
  }
}

// 5. Report.
console.log(`Total message keys: ${allKeys.length}`)
console.log(`Used keys: ${usedCount}`)
console.log(`Orphan keys: ${orphans.length}`)
if (verbose && usedCount > 0) {
  console.log(`(use rate: ${((usedCount / allKeys.length) * 100).toFixed(1)}%)`)
}
if (orphans.length > 0) {
  console.log('\nOrphan keys (defined in messages/en.json but not referenced in src/):')
  for (const k of orphans.sort()) console.log(`  ${k}`)
  console.log(
    `\nNote: a key is counted as used when it appears as a quoted literal in src/ (record-fed ` +
      `m[variable]() lookups), or shares a prefix with a template-literal lookup. Commented-out ` +
      `occurrences would also count — review before deleting.`
  )
  process.exit(1)
}
