#!/usr/bin/env node
// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md
//
// Regenerates THIRD-PARTY-NOTICES.txt, the file Astrail shows in Settings →
// About to satisfy the attribution clauses of the code it ships.
//
//   node scripts/third-party-notices.mjs           rewrite the file
//   node scripts/third-party-notices.mjs --check    fail if it is out of date
//
// Three dependency sets feed it: the Rust crates linked into the app, the npm
// packages in the frontend's production tree, and the NuGet packages inside the
// cputemp sidecar. License texts are read from the packages themselves so the
// file says what upstream says; anything that cannot be read is listed in the
// tables below or in scripts/notices-static.txt, and a package that matches
// neither aborts the run instead of being silently dropped.

import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

const REPO = path.resolve(import.meta.dirname, '..')
const OUT = path.join(REPO, 'THIRD-PARTY-NOTICES.txt')
const STATIC = path.join(REPO, 'scripts', 'notices-static.txt')
const LICENSE_FILE = /^(licen[sc]es?|copying|copyright|unlicense)([-_. ].*)?$/i
const NOTICE_FILE = /^notice([-_. ].*)?$/i

// Which side of an `OR` to reproduce. Astrail is GPL-3.0-only, so anything here
// is compatible; the order just prefers the shortest, least demanding text.
const PREFERENCE = [
  'MIT', '0BSD', 'ISC', 'BSD-2-Clause', 'BSD-3-Clause', 'Zlib', 'MIT-0',
  'Unlicense', 'CC0-1.0', 'Apache-2.0', 'MPL-2.0', 'Unicode-3.0',
  'CDLA-Permissive-2.0',
]

// Packages that ship no license file at all. The holder is what upstream states
// in its own repository, or — when that has no license file either — in the
// package metadata; the text is the canonical one for the declared identifier.
// `null` means the license has no copyright line to fill in.
const NO_LICENSE_FILE = {
  'cargo:webview2-com': ['MIT', 'Copyright (c) 2021 Bill Avery'],
  'cargo:webview2-com-sys': ['MIT', 'Copyright (c) 2021 Bill Avery'],
  'cargo:webview2-com-macros': ['MIT', 'Copyright (c) 2021 Bill Avery'],
  'cargo:unic-char-property': ['MIT', 'Copyright (c) The UNIC Project Developers'],
  'cargo:unic-char-range': ['MIT', 'Copyright (c) The UNIC Project Developers'],
  'cargo:unic-common': ['MIT', 'Copyright (c) The UNIC Project Developers'],
  'cargo:unic-ucd-ident': ['MIT', 'Copyright (c) The UNIC Project Developers'],
  'cargo:unic-ucd-version': ['MIT', 'Copyright (c) The UNIC Project Developers'],
  'cargo:alloc-stdlib': ['BSD-3-Clause', 'Copyright (c) 2016 Dropbox, Inc.\nAll rights reserved.'],
  'cargo:crc-catalog': ['MIT', 'Copyright (c) Akhil Velagapudi'],
  'cargo:nvml-wrapper': ['MIT', 'Copyright (c) 2017 Jarek Samic'],
  'cargo:nvml-wrapper-sys': ['MIT', 'Copyright (c) 2017 Jarek Samic'],
  'cargo:pelite-macros': ['MIT', 'Copyright (c) 2016-2018 Casper'],
  'cargo:selectors': ['MPL-2.0', null],
  'npm:@next/env': ['MIT', 'Copyright (c) 2025 Vercel, Inc.'],
  'npm:@next/swc-win32-x64-msvc': ['MIT', 'Copyright (c) 2025 Vercel, Inc.'],
  'npm:client-only': ['MIT', 'Copyright (c) 2025 Vercel, Inc.'],
  'npm:html-parse-stringify': ['MIT', 'Copyright (c) Henrik Joreteg'],
  'npm:@tauri-apps/plugin-dialog': ['MIT', 'Copyright (c) 2017 - Present Tauri Apps Contributors'],
  'npm:@tauri-apps/plugin-process': ['MIT', 'Copyright (c) 2017 - Present Tauri Apps Contributors'],
  'npm:@tauri-apps/plugin-updater': ['MIT', 'Copyright (c) 2017 - Present Tauri Apps Contributors'],
}

// The NuGet side is described here rather than read from ~/.nuget, so that the
// notices can be regenerated from a clone alone. A package that appears in
// packages.lock.json without an entry here stops the run.
const NUGET = {
  // The MPL asks that recipients be told where the source is, so these point at
  // the commit each package was built from, as recorded in its .nuspec.
  'LibreHardwareMonitorLib': { license: 'MPL-2.0', source: 'https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/tree/0c05d35b8fb013256efa11ada3266fae843aef81' },
  'BlackSharp.Core': { license: 'MPL-2.0', source: 'https://github.com/Blacktempel/BlackSharp/tree/35f908a3d5cefaf8d4557709273d25e166593109' },
  'DiskInfoToolkit': { license: 'MPL-2.0', source: 'https://github.com/Blacktempel/DiskInfoToolkit/tree/3c7f43001e66f478e20587f5d8e7edd8ec7e2806' },
  'RAMSPDToolkit-NDD': { license: 'MPL-2.0', source: 'https://github.com/Blacktempel/RAMSPDToolkit/tree/713038f919d85397214e17644d78534dc15a2fd2' },
  'HidSharp': { license: 'Apache-2.0', text: 'hidsharp' },
  'Mono.Posix.NETStandard': { license: 'MIT', holder: 'Copyright (c) Microsoft Corporation' },
  'System.CodeDom': { license: 'MIT', text: 'dotnet-mit' },
  'System.IO.Ports': { license: 'MIT', text: 'dotnet-mit' },
  'System.Management': { license: 'MIT', text: 'dotnet-mit' },
  'System.Threading.AccessControl': { license: 'MIT', text: 'dotnet-mit' },
  'runtime.native.System.IO.Ports': { license: 'MIT', text: 'dotnet-mit' },
}

// Locked NuGet packages whose code never reaches a user's machine.
const NUGET_SKIP = [
  ['Microsoft.NET.ILLink.Tasks', 'trimmer, runs during the build'],
  [/^runtime\.(android|linux|maccatalyst|osx)-/, 'native assets for platforms Astrail does not target'],
]

const CANONICAL = {
  'MIT': `MIT License

%COPYRIGHT%

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.`,
  'BSD-3-Clause': `%COPYRIGHT%

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its contributors
   may be used to endorse or promote products derived from this software
   without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.`,
}

const die = (msg) => { console.error(`third-party-notices: ${msg}`); process.exit(1) }
const clean = (s) => s.replace(/\r\n/g, '\n').replace(/\uFEFF/g, '').replace(/[ \t]+$/gm, '').replace(/^\n+|\n+$/g, '')

function readStatic() {
  const blocks = new Map()
  let key = null
  let buf = []
  for (const line of clean(fs.readFileSync(STATIC, 'utf8')).split('\n')) {
    const m = /^@@ ([a-z0-9.-]+)$/.exec(line)
    if (m) {
      if (key) blocks.set(key, buf.join('\n'))
      key = m[1]
      buf = []
    } else if (key) {
      buf.push(line)
    }
  }
  if (key) blocks.set(key, buf.join('\n'))
  return blocks
}

const statics = readStatic()
const staticText = (key) => {
  const t = statics.get(key)
  if (!t) die(`scripts/notices-static.txt has no block "@@ ${key}"`)
  return clean(t)
}

// --- the pool of unique license texts -------------------------------------

const pool = new Map() // text -> { n, users: [] }

function useText(text, user) {
  const t = clean(text)
  if (!t) die(`empty license text for ${user}`)
  let entry = pool.get(t)
  if (!entry) {
    entry = { n: pool.size + 1, users: [] }
    pool.set(t, entry)
  }
  entry.users.push(user)
  return entry.n
}

/** Guess the SPDX identifier of a license text, to pair files with ids. */
function detect(text) {
  const t = text.replace(/\s+/g, ' ')
  if (/Apache License Version 2\.0/i.test(t)) return 'Apache-2.0'
  if (/Mozilla Public License Version 2\.0/i.test(t)) return 'MPL-2.0'
  if (/GNU LESSER GENERAL PUBLIC LICENSE Version 2\.1/i.test(t)) return 'LGPL-2.1'
  if (/UNICODE LICENSE|Unicode License V3/i.test(t)) return 'Unicode-3.0'
  if (/Community Data License Agreement/i.test(t)) return 'CDLA-Permissive-2.0'
  if (/CC0 1\.0 Universal/i.test(t)) return 'CC0-1.0'
  if (/SIL OPEN FONT LICENSE/i.test(t)) return 'OFL-1.1'
  if (/free and unencumbered software released into the public domain/i.test(t)) return 'Unlicense'
  if (/altered source versions must be plainly marked/i.test(t)) return 'Zlib'
  if (/Permission to use, copy, modify, and\/?o?r? ?distribute/i.test(t)) {
    return /copyright notice and this permission notice appear in all copies/i.test(t) ? 'ISC' : '0BSD'
  }
  if (/Redistribution and use in source and binary forms/i.test(t)) {
    return /Neither the name/i.test(t) ? 'BSD-3-Clause' : 'BSD-2-Clause'
  }
  if (/Permission is hereby granted, free of charge/i.test(t)) return 'MIT'
  return null
}

/** Reduce an SPDX expression to the identifiers whose text must be reproduced. */
function chosenIds(expr, who) {
  const rank = (id) => {
    const i = PREFERENCE.indexOf(id)
    return i === -1 ? PREFERENCE.length : i
  }
  const ids = []
  for (const term of expr.replace(/[()]/g, '').replace(/\//g, ' OR ').split(/ AND /i)) {
    const options = term.split(/ OR /i).map((s) => s.trim().replace(/ WITH .*/i, '')).filter(Boolean)
    if (!options.length) die(`cannot parse the license expression of ${who}: ${expr}`)
    ids.push(options.reduce((a, b) => (rank(b) < rank(a) ? b : a)))
  }
  return [...new Set(ids)]
}

/** The canonical text of `id`, either reused from the tree or filled in here. */
function canonical(id, holder, who) {
  if (CANONICAL[id]) {
    if (!holder) die(`${who}: the ${id} template needs a copyright holder`)
    return CANONICAL[id].replace('%COPYRIGHT%', holder)
  }
  for (const text of pool.keys()) if (detect(text) === id && !/^Copyright/im.test(text)) return text
  die(`${who}: no text available for ${id}; add one to scripts/notices-static.txt`)
}

/** Texts to reproduce for one package, given its files on disk. */
function textsFor(kind, name, version, dir, expr, notes) {
  const who = `${name} ${version}`
  const override = NO_LICENSE_FILE[`${kind}:${name}`]
  const ids = chosenIds(expr, who)
  const found = []

  let files = []
  try {
    files = fs.readdirSync(dir, { withFileTypes: true })
      .filter((e) => e.isFile() && (LICENSE_FILE.test(e.name) || NOTICE_FILE.test(e.name)))
      .map((e) => e.name)
      .sort()
  } catch {
    die(`${who}: cannot read ${dir}`)
  }
  // An SPDX document describes a license, it is not one.
  files = files.filter((f) => !/\.spdx$/i.test(f))

  const read = (f) => clean(fs.readFileSync(path.join(dir, f), 'utf8'))
  const texts = files.map((f) => ({ file: f, text: read(f), id: detect(read(f)) }))
  const notice = texts.find((t) => NOTICE_FILE.test(t.file) && ids.includes('Apache-2.0'))

  if (override) {
    if (texts.length) die(`${who} now ships ${files.join(', ')}; drop its NO_LICENSE_FILE entry`)
    found.push(canonical(override[0], override[1], who))
    notes.push(override[1]
      ? 'the package ships no license file; the canonical text of the declared '
        + 'license is reproduced, with the copyright holder the package states'
      : 'the package ships no license file; the canonical text of the declared '
        + 'license is reproduced')
  } else if (!texts.length) {
    die(`${who} (${expr}) has no license file in ${dir}; add a NO_LICENSE_FILE entry`)
  } else if (texts.length === 1 && ids.length === 1) {
    found.push(texts[0].text)
  } else {
    const missing = []
    for (const id of ids) {
      const hit = texts.find((t) => t.id === id)
        ?? texts.find((t) => t.file.toUpperCase().includes(id.split('-')[0].toUpperCase()))
      if (!hit) missing.push(id)
      else if (!found.includes(hit.text)) found.push(hit.text)
    }
    if (!found.length) die(`${who}: none of ${files.join(', ')} matches ${expr}`)
    // A package can declare a license for a part of itself that it does not
    // ship, or cover several ids with one file. Say so rather than quietly
    // dropping the id from the list.
    if (missing.length) notes.push(`the package declares ${expr} but ships no separate text for ${missing.join(' or ')}`)
  }
  if (notice && !found.includes(notice.text)) found.push(notice.text)
  return found
}

// --- the three dependency trees -------------------------------------------

function rustPackages() {
  const run = (extra) => JSON.parse(execFileSync('cargo', [
    'metadata', '--locked', '--format-version', '1',
    '--filter-platform', 'x86_64-pc-windows-msvc', ...extra,
  ], { cwd: path.join(REPO, 'src-tauri'), maxBuffer: 256 * 1024 * 1024, windowsHide: true }))

  let meta
  try {
    // Offline keeps a local run instant; a cold CI checkout has no registry
    // index yet, so fall back to letting cargo fetch it.
    meta = run(['--offline'])
  } catch {
    try {
      meta = run([])
    } catch (e) {
      die(`cargo metadata failed: ${e.message}`)
    }
  }
  const pkgs = new Map(meta.packages.map((p) => [p.id, p]))
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]))
  const workspace = new Set(meta.workspace_members)
  const seen = new Set()
  const stack = [...workspace]
  while (stack.length) {
    const id = stack.pop()
    if (seen.has(id)) continue
    seen.add(id)
    // Normal dependencies only: build scripts and dev-dependencies do not ship.
    for (const dep of nodes.get(id).deps) {
      if (dep.dep_kinds.some((k) => k.kind === null)) stack.push(dep.pkg)
    }
  }
  return [...seen]
    .filter((id) => !workspace.has(id))
    .map((id) => pkgs.get(id))
    .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version))
    .map((p) => ({
      name: p.name,
      version: p.version,
      expr: p.license ?? die(`crate ${p.name} declares no license`),
      dir: path.dirname(p.manifest_path),
      repository: p.repository,
      authors: p.authors,
    }))
}

function npmPackages() {
  const lock = JSON.parse(fs.readFileSync(path.join(REPO, 'package-lock.json'), 'utf8'))
  const out = []
  for (const [key, entry] of Object.entries(lock.packages)) {
    if (!key.startsWith('node_modules/') || entry.dev || entry.devOptional) continue
    const dir = path.join(REPO, key)
    // Optional packages for other platforms are in the lock file but never
    // installed here, and their code cannot end up in a Windows build.
    if (!fs.existsSync(dir)) continue
    out.push({
      name: key.slice(key.lastIndexOf('node_modules/') + 'node_modules/'.length),
      version: entry.version,
      expr: entry.license ?? die(`npm package ${key} declares no license`),
      dir,
      repository: null,
    })
  }
  return out.sort((a, b) => a.name.localeCompare(b.name))
}

function nugetPackages() {
  const lock = JSON.parse(fs.readFileSync(
    path.join(REPO, 'src-tauri/sidecar/cputemp/packages.lock.json'), 'utf8'))
  const resolved = new Map()
  for (const deps of Object.values(lock.dependencies)) {
    for (const [id, dep] of Object.entries(deps)) resolved.set(id, dep.resolved)
  }
  const out = []
  for (const [id, version] of [...resolved].sort((a, b) => a[0].localeCompare(b[0]))) {
    if (NUGET_SKIP.some(([m]) => (typeof m === 'string' ? m === id : m.test(id)))) continue
    const info = NUGET[id]
    if (!info) die(`NuGet package ${id} is locked but missing from the NUGET table`)
    out.push({ name: id, version, ...info })
  }
  return out
}

// --- rendering -------------------------------------------------------------

function wrap(text, width, indent) {
  const lines = []
  let line = ''
  for (const word of text.split(' ')) {
    if (line && (line + ' ' + word).length > width) {
      lines.push(line)
      line = word
    } else {
      line = line ? `${line} ${word}` : word
    }
  }
  if (line) lines.push(line)
  return lines.map((l, i) => (i ? indent + l : l)).join('\n')
}

const heading = (title) => `${title}\n${'-'.repeat(title.length)}\n`

function renderTree(kind, packages, intro) {
  const lines = [heading(`${intro.title} (${packages.length})`), wrap(intro.body, 78, ''), '']
  for (const pkg of packages) {
    const notes = []
    const refs = textsFor(kind, pkg.name, pkg.version, pkg.dir, pkg.expr, notes)
      .map((t) => useText(t, `${pkg.name} ${pkg.version}`))
    lines.push(`  ${pkg.name} ${pkg.version} — ${pkg.expr} — ${refs.map((n) => `text ${n}`).join(', ')}`)
    if (/MPL-2\.0|LGPL/.test(pkg.expr) && pkg.repository) lines.push(`      source: ${pkg.repository}`)
    for (const note of notes) lines.push(`      ${note}`)
  }
  return lines.join('\n')
}

function renderNuget(packages) {
  const lines = [heading(`.NET packages in the cputemp sidecar (${packages.length})`), wrap(
    'The cputemp sidecar reads CPU and GPU sensors. It is published self-contained, '
    + 'so these packages and the .NET runtime itself are inside cputemp.exe.', 78, ''), '']
  for (const pkg of packages) {
    const text = pkg.text
      ? staticText(pkg.text)
      : canonical(pkg.license, pkg.holder, `${pkg.name} ${pkg.version}`)
    const n = useText(text, `${pkg.name} ${pkg.version}`)
    lines.push(`  ${pkg.name} ${pkg.version} — ${pkg.license} — text ${n}`)
    if (pkg.source) lines.push(`      source: ${pkg.source}`)
  }
  return lines.join('\n')
}

function renderBundled() {
  const lines = [heading('Bundled binaries and assets'), wrap(
    'These are shipped as they are, next to the Astrail executable or inside it.', 78, ''), '']
  const item = (title, body, texts) => {
    lines.push(`  ${title}`)
    lines.push('      ' + wrap(body, 72, '      '))
    lines.push(`      ${texts.map(([label, key]) => `${label}: text ${useText(staticText(key), title)}`).join(', ')}`)
    lines.push('')
  }
  item('PresentMon 2.4.1 (Intel Corporation) — MIT',
    'Bundled as PresentMon.exe. Astrail runs it to sample frame times; it is '
    + 'downloaded and hash-pinned by scripts/fetch-binaries.ps1. '
    + 'Source: https://github.com/GameTechDev/PresentMon',
    [['license', 'presentmon']])
  item('.NET 8 runtime (.NET Foundation and Contributors) — MIT',
    'Embedded in cputemp.exe by `dotnet publish --self-contained`. The second '
    + 'text is the runtime\'s own third-party notices, passed on unchanged. '
    + 'Source: https://github.com/dotnet/runtime',
    [['license', 'dotnet-mit'], ['third-party notices', 'dotnet-third-party']])
  item('PawnIO modules (namazso and contributors) — LGPL-2.1',
    'LibreHardwareMonitorLib embeds the compiled PawnIo.*.bin modules it loads '
    + 'into the PawnIO driver to read hardware sensors, so they travel inside '
    + 'cputemp.exe. Source: https://github.com/namazso/PawnIO.Modules',
    [['license', 'lgpl-2.1']])
  item('Oxanium (The Oxanium Project Authors) — OFL-1.1',
    'Astrail\'s display typeface, self-hosted as a webfont by next/font. '
    + 'Copyright 2019 The Oxanium Project Authors '
    + '(https://github.com/sevmeyer/oxanium).',
    [['license', 'ofl-1.1']])
  item('Source Code Pro (Adobe Systems Incorporated) — OFL-1.1',
    'Astrail\'s monospace typeface, self-hosted as a webfont by next/font. '
    + 'Copyright 2010, 2012 Adobe Systems Incorporated (http://www.adobe.com/), '
    + 'with Reserved Font Name \'Source\'. Source is a trademark of Adobe Systems '
    + 'Incorporated in the United States and/or other countries.',
    [['license', 'ofl-1.1']])
  return lines.join('\n').replace(/\n+$/, '')
}

function renderTexts() {
  const lines = [heading('License texts')]
  for (const [text, entry] of pool) {
    const users = [...new Set(entry.users)].join(', ')
    lines.push(`Text ${entry.n}`)
    lines.push('~'.repeat(`Text ${entry.n}`.length))
    lines.push(wrap(`Applies to: ${users}`, 78, '    '))
    lines.push('')
    lines.push(text)
    lines.push('')
  }
  return lines.join('\n').replace(/\n+$/, '')
}

// --- assembly --------------------------------------------------------------

const rust = rustPackages()
const npm = npmPackages()
const nuget = nugetPackages()

const body = [
  renderTree('cargo', rust, {
    title: 'Rust crates',
    body: 'The normal-dependency closure of src-tauri for x86_64-pc-windows-msvc. '
      + 'Build-time-only and test-only crates are left out because their code is '
      + 'not part of what Astrail ships.',
  }),
  renderTree('npm', npm, {
    title: 'npm packages',
    body: 'The production dependency tree of the frontend, as installed on Windows. '
      + 'Astrail ships a static export, so most of these only run while the '
      + 'interface is being built and never reach a user; they are listed anyway '
      + 'rather than guessed at. Where a package declares more than one license '
      + 'but ships the text of only some of them, that is noted next to the '
      + 'package and the declared expression is reproduced above.',
  }),
  renderNuget(nuget),
  renderBundled(),
  renderTexts(),
]

const header = `Astrail — third-party notices
============================

Astrail is Copyright (C) 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev) and is
distributed under the GNU General Public License version 3 with the additional
terms in ADDITIONAL-TERMS.md. The components listed here are the work of other
people and keep their own licenses, which are reproduced in full at the end of
this file.

${wrap('Generated by scripts/third-party-notices.mjs from Cargo.lock, '
  + 'package-lock.json and packages.lock.json. Do not edit by hand: run '
  + '`node scripts/third-party-notices.mjs` instead.', 78, '')}

Contents:
  Rust crates .......................... ${rust.length}
  npm packages ......................... ${npm.length}
  .NET packages in the cputemp sidecar . ${nuget.length}
  Bundled binaries and assets
  License texts
`

const out = `${header}\n\n${body.join('\n\n\n')}\n`

if (process.argv.includes('--check')) {
  const current = fs.existsSync(OUT) ? fs.readFileSync(OUT, 'utf8') : ''
  if (current !== out) {
    die('THIRD-PARTY-NOTICES.txt is out of date. Run `node scripts/third-party-notices.mjs`.')
  }
  console.log(`third-party-notices: up to date (${pool.size} license texts)`)
} else {
  fs.writeFileSync(OUT, out, 'utf8')
  console.log(`third-party-notices: wrote THIRD-PARTY-NOTICES.txt `
    + `(${rust.length} crates, ${npm.length} npm, ${nuget.length} NuGet, ${pool.size} license texts)`)
}
