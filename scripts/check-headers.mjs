// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// Checks that every hand-written source file starts with the SPDX header.
//
// GPL-3.0 section 7 lets the additional terms live in one file as long as the
// sources say where they are, so the header is three lines and the third one
// is the pointer. Run `node scripts/check-headers.mjs --fix` to add it where it
// is missing; `npm run check` and CI run it without --fix.
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

const REPO = path.resolve(path.dirname(new URL(import.meta.url).pathname.slice(1)), '..')
const FIX = process.argv.includes('--fix')

const HEADER = [
  'SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)',
  'SPDX-License-Identifier: GPL-3.0-only',
  'Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md',
]

// How each language spells a line comment. CSS has no line comment, so its
// three lines are three block comments.
const COMMENT = {
  '.ts': '// ', '.tsx': '// ', '.js': '// ', '.mjs': '// ', '.cjs': '// ',
  '.rs': '// ', '.cs': '// ',
  '.ps1': '# ',
  '.nsi': '; ',
  '.css': ['/* ', ' */'],
}

/** Files that are not ours to label. */
const SKIP = [
  /^src-tauri\/gen\//,
  /^src-tauri\/binaries\//,
  /\.d\.ts$/,
]

function sources() {
  // `--others` so a source file that is not committed yet is checked too.
  const out = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], { cwd: REPO, encoding: 'utf8' })
  return out.split('\0')
    .filter(Boolean)
    .filter((f) => COMMENT[path.extname(f)] && !SKIP.some((re) => re.test(f)))
    .sort()
}

/** The header as it should look at the top of this file. */
function render(ext) {
  const c = COMMENT[ext]
  return Array.isArray(c)
    ? HEADER.map((l) => `${c[0]}${l}${c[1]}`).join('\n')
    : HEADER.map((l) => `${c}${l}`).join('\n')
}

/** Strip whatever comment syntax a line uses, so the check is not per-language. */
function bare(line) {
  return line
    .replace(/^\s*(\/\/+|#+|;+|\/\*|\*)\s?/, '')
    .replace(/\s*\*\/\s*$/, '')
    .trim()
}

const missing = []
for (const file of sources()) {
  const full = path.join(REPO, file)
  const text = fs.readFileSync(full, 'utf8')
  // A shebang has to stay on line 1, so the header goes under it.
  const shebang = text.startsWith('#!') ? `${text.slice(0, text.indexOf('\n') + 1)}` : ''
  const rest = text.slice(shebang.length)
  const head = rest.split('\n', HEADER.length).map(bare)
  if (HEADER.every((want, i) => head[i] === want)) continue

  if (!FIX) {
    missing.push(file)
    continue
  }
  fs.writeFileSync(full, `${shebang}${render(path.extname(file))}\n\n${rest.replace(/^\n+/, '')}`, 'utf8')
  console.log(`check-headers: added header to ${file}`)
}

if (missing.length) {
  console.error(`check-headers: ${missing.length} file(s) without the SPDX header:`)
  for (const f of missing) console.error(`  ${f}`)
  console.error('Run `node scripts/check-headers.mjs --fix`.')
  process.exit(1)
}
if (!FIX) console.log(`check-headers: ok (${sources().length} files)`)
