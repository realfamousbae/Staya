#!/usr/bin/env node
// Generates self-hosted SVG stat cards for the README (adapted from Zuno).
// Usage: node scripts/repo-stats-badges.mjs <output-dir>
// Reads only tracked files via git; no dependencies besides Node 22.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, extname, join } from 'node:path';

const outputDir = process.argv[2] ?? 'repo-stats';
const git = (...args) =>
  execFileSync('git', args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }).trim();

// ---------------------------------------------------------------------------
// Collect statistics
// ---------------------------------------------------------------------------

const languages = [
  { name: 'Rust', color: '#f97316', ext: ['.rs'] },
  { name: 'Swift', color: '#f43f5e', ext: ['.swift'] },
  { name: 'Kotlin', color: '#8b5cf6', ext: ['.kt', '.kts'] },
  { name: 'Markdown', color: '#a855f7', ext: ['.md'] },
  { name: 'Config', color: '#14b8a6', ext: ['.toml', '.yml', '.yaml', '.json', '.xml', '.xcconfig', '.plist', '.pbxproj', '.properties'] },
  { name: 'Shell', color: '#22c55e', ext: ['.sh'] },
];
const other = { name: 'Other', color: '#94a3b8' };
const codeExtensions = new Set(['.rs', '.swift', '.kt']);
// Lockfiles, generated and vendored files would only distort the numbers.
const skipped = (path) =>
  path === 'Cargo.lock' || path === 'LICENSE' || /(^|\/)gradlew(\.bat)?$/.test(path) || path.endsWith('.jar');
const isTestFile = (path) =>
  /(^|\/)tests\//.test(path) || /\/src\/test\//.test(path) || /Tests?\.swift$/.test(path);

const files = git('ls-files', '-z').split('\0').filter(Boolean);
const languageLines = new Map();
const codeByLanguage = new Map();
const textFiles = [];
let totalLines = 0;
let codeFiles = 0;
let codeLines = 0;
let testFiles = 0;
let testLines = 0;
let testCases = 0;
let unsafeBlocks = 0;
let todos = 0;

for (const path of files) {
  if (skipped(path) || !existsSync(path)) continue;
  const buffer = readFileSync(path);
  if (buffer.subarray(0, 8000).includes(0)) continue;
  let lines = 0;
  for (const byte of buffer) if (byte === 10) lines += 1;
  if (buffer.length > 0 && buffer[buffer.length - 1] !== 10) lines += 1;

  const ext = extname(path);
  const language = languages.find((entry) => entry.ext.includes(ext)) ?? other;
  languageLines.set(language, (languageLines.get(language) ?? 0) + lines);
  textFiles.push({ path, lines });
  totalLines += lines;

  if (codeExtensions.has(ext)) {
    const text = buffer.toString('utf8');
    codeFiles += 1;
    codeLines += lines;
    codeByLanguage.set(language.name, (codeByLanguage.get(language.name) ?? 0) + lines);
    todos += (text.match(/\b(TODO|FIXME|HACK)\b/g) ?? []).length;
    // #[test] in Rust (proptest! cases carry it too), @Test in Kotlin, func test… in Swift.
    testCases += (text.match(/#\[test\]|@Test\b|func test[A-Z]\w*\(/g) ?? []).length;
    if (ext === '.rs') unsafeBlocks += (text.match(/\bunsafe\s*(\{|fn\b|impl\b)/g) ?? []).length;
    if (isTestFile(path)) {
      testFiles += 1;
      testLines += lines;
    }
  }
}

// Workspace crates and external crates from Cargo.lock (packages with a registry source).
const lock = existsSync('Cargo.lock') ? readFileSync('Cargo.lock', 'utf8') : '';
const lockPackages = lock.split('[[package]]').slice(1);
const externalCrates = lockPackages.filter((entry) => /\nsource = /.test(entry)).length;
const workspaceCrates = lockPackages.length - externalCrates;
const cargo = readFileSync('Cargo.toml', 'utf8');
const version = cargo.match(/\[workspace\.package\][\s\S]*?\nversion = "([^"]+)"/)?.[1] ?? '0.0.0';
const protocolVersion =
  (existsSync('docs/protocol.md') ? readFileSync('docs/protocol.md', 'utf8') : '').match(
    /Статус: \*\*(v[\d.]+)\*\*/,
  )?.[1] ?? '—';
const plan = existsSync('docs/PLAN.md') ? readFileSync('docs/PLAN.md', 'utf8') : '';
const tasksDone = (plan.match(/^- \[x\]/gm) ?? []).length;
const tasksTotal = tasksDone + (plan.match(/^- \[ \]/gm) ?? []).length;

const commitDates = git('log', '--format=%ct').split('\n').map(Number);
const now = Date.now() / 1000;
const day = 86_400;
const firstCommit = Math.min(...commitDates);
const authors = new Set(git('log', '--format=%aN').split('\n'));
const sourceLines = codeLines - testLines;
const largest = [...textFiles].sort((a, b) => b.lines - a.lines).slice(0, 5);

const stats = {
  generatedAt: new Date().toISOString(),
  commit: git('rev-parse', 'HEAD'),
  version,
  files: files.length,
  lines: totalLines,
  code: {
    files: codeFiles,
    lines: codeLines,
    sourceLines,
    averageLines: Math.round(codeLines / Math.max(codeFiles, 1)),
    rust: codeByLanguage.get('Rust') ?? 0,
    swift: codeByLanguage.get('Swift') ?? 0,
    kotlin: codeByLanguage.get('Kotlin') ?? 0,
  },
  tests: {
    cases: testCases,
    files: testFiles,
    lines: testLines,
  },
  languages: [...languageLines]
    .sort((a, b) => b[1] - a[1])
    .map(([language, lines]) => ({ name: language.name, color: language.color, lines })),
  largest,
  crates: { workspace: workspaceCrates, external: externalCrates },
  unsafeBlocks,
  protocolVersion,
  plan: { done: tasksDone, total: tasksTotal },
  docs: { markdown: files.filter((path) => path.endsWith('.md')).length },
  todos,
  commits: {
    total: commitDates.length,
    last7Days: commitDates.filter((time) => now - time < 7 * day).length,
    last30Days: commitDates.filter((time) => now - time < 30 * day).length,
    lastCommit: new Date(Math.max(...commitDates) * 1000).toISOString(),
  },
  contributors: authors.size,
  ageDays: Math.max(1, Math.ceil((now - firstCommit) / day)),
};

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

async function loadFonts() {
  const sources = [
    ['Inter', 400, '@fontsource/inter/files/inter-latin-400-normal.woff2'],
    ['Inter', 600, '@fontsource/inter/files/inter-latin-600-normal.woff2'],
    ['Inter', 800, '@fontsource/inter/files/inter-latin-800-normal.woff2'],
    [
      'JetBrains Mono',
      500,
      '@fontsource/jetbrains-mono/files/jetbrains-mono-latin-500-normal.woff2',
    ],
  ];
  const faces = [];
  for (const [family, weight, path] of sources) {
    try {
      const response = await fetch(`https://cdn.jsdelivr.net/npm/${path}`);
      if (!response.ok) throw new Error(String(response.status));
      const data = Buffer.from(await response.arrayBuffer()).toString('base64');
      faces.push(
        `@font-face{font-family:'${family}';font-weight:${weight};src:url(data:font/woff2;base64,${data}) format('woff2')}`,
      );
    } catch (error) {
      console.warn(`Font ${family} ${weight} unavailable, using system fallback: ${error.message}`);
    }
  }
  return faces.join('');
}

const fonts = await loadFonts();
const number = (value) => value.toLocaleString('en-US');
const compact = (value) =>
  value >= 10_000 ? `${(value / 1000).toFixed(value >= 100_000 ? 0 : 1)}k` : number(value);
const escape = (text) =>
  String(text)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
const days = (n) => `${n} day${n === 1 ? '' : 's'}`;
const truncate = (text, max) => (text.length > max ? `…${text.slice(-(max - 1))}` : text);

const style = `${fonts}
:root{--bg:#ffffff;--bg2:#f6f8fa;--border:#d0d7de;--fg:#1f2328;--muted:#656d76;--track:#eaeef2}
@media (prefers-color-scheme:dark){:root{--bg:#161b22;--bg2:#0d1117;--border:#30363d;--fg:#e6edf3;--muted:#8d96a0;--track:#21262d}}
text{font-family:'Inter',-apple-system,'Segoe UI',Helvetica,Arial,sans-serif;fill:var(--fg)}
.label{font-size:12px;font-weight:600;letter-spacing:.08em;fill:var(--muted)}
.value{font-size:38px;font-weight:800;letter-spacing:-.02em}
.sub{font-size:13px;font-weight:400;fill:var(--muted)}
.mono{font-family:'JetBrains Mono',ui-monospace,SFMono-Regular,Menlo,monospace;font-weight:500}
.title{font-size:22px;font-weight:800;letter-spacing:-.01em}
.small{font-size:12px;font-weight:400;fill:var(--muted)}`;

function frame(width, height, accent, body) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="img">
<style>${style}</style>
<defs>
<linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="var(--bg)"/><stop offset="1" stop-color="var(--bg2)"/></linearGradient>
<linearGradient id="glow" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="${accent}"/><stop offset="1" stop-color="${accent}" stop-opacity="0"/></linearGradient>
<clipPath id="card"><rect x=".5" y=".5" width="${width - 1}" height="${height - 1}" rx="16"/></clipPath>
</defs>
<rect x=".5" y=".5" width="${width - 1}" height="${height - 1}" rx="16" fill="url(#bg)" stroke="var(--border)"/>
<rect x="0" y="0" width="${width}" height="4" fill="url(#glow)" clip-path="url(#card)"/>
${body}
</svg>
`;
}

const tileWidth = 290;
const tileHeight = 132;
const tileGap = 15;

function tile({ label, value, sub, accent, mono = false, valueSize }) {
  const size = valueSize ? ` style="font-size:${valueSize}px"` : '';
  const center = tileWidth / 2;
  return {
    accent,
    body: `<g text-anchor="middle">
<text x="${center}" y="34" class="label"><tspan style="fill:${accent}">●</tspan>  ${escape(label.toUpperCase())}</text>
<text x="${center}" y="${valueSize ? 78 : 82}" class="value${mono ? ' mono' : ''}"${size}>${escape(value)}</text>
<text x="${center}" y="110" class="sub">${escape(sub)}</text></g>`,
  };
}

// Renders tiles as one grid so its outer width matches the full-width cards exactly.
function tileGrid(tiles, columns = 3) {
  const rows = Math.ceil(tiles.length / columns);
  const width = columns * tileWidth + (columns - 1) * tileGap;
  const height = rows * tileHeight + (rows - 1) * tileGap;
  const cells = tiles
    .map(({ accent, body }, index) => {
      const x = (index % columns) * (tileWidth + tileGap);
      const y = Math.floor(index / columns) * (tileHeight + tileGap);
      return `<g transform="translate(${x} ${y})">
<defs>
<linearGradient id="glow${index}" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="${accent}"/><stop offset="1" stop-color="${accent}" stop-opacity="0"/></linearGradient>
<clipPath id="card${index}"><rect x=".5" y=".5" width="${tileWidth - 1}" height="${tileHeight - 1}" rx="16"/></clipPath>
</defs>
<rect x=".5" y=".5" width="${tileWidth - 1}" height="${tileHeight - 1}" rx="16" fill="url(#bg)" stroke="var(--border)"/>
<rect width="${tileWidth}" height="4" fill="url(#glow${index})" clip-path="url(#card${index})"/>
${body}
</g>`;
    })
    .join('\n');
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="img">
<style>${style}</style>
<defs><linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="var(--bg)"/><stop offset="1" stop-color="var(--bg2)"/></linearGradient></defs>
${cells}
</svg>
`;
}

function overview() {
  const width = 900;
  const cells = [
    ['LINES', compact(stats.lines), `${number(stats.lines)} in text files`],
    ['FILES', number(stats.files), `${number(stats.code.files)} Rust/Swift/Kotlin files`],
    ['COMMITS', number(stats.commits.total), `${stats.commits.last30Days} in the last 30 days`],
    ['CONTRIBUTORS', number(stats.contributors), `over ${days(stats.ageDays)}`],
  ];
  const cellWidth = (width - 48) / cells.length;
  const cellsSvg = cells
    .map(
      ([label, value, sub], index) => `<g transform="translate(${24 + index * cellWidth} 0)">
${index ? `<line x1="-12" y1="84" x2="-12" y2="148" stroke="var(--border)"/>` : ''}
<text x="0" y="96" class="label">${label}</text>
<text x="0" y="136" class="value">${escape(value)}</text>
<text x="0" y="160" class="sub">${escape(sub)}</text></g>`,
    )
    .join('\n');

  const barWidth = width - 48;
  const total = stats.languages.reduce((sum, entry) => sum + entry.lines, 0);
  let offset = 0;
  const segments = stats.languages
    .map((entry) => {
      const segmentWidth = (entry.lines / total) * barWidth;
      const rect = `<rect x="${24 + offset}" y="190" width="${Math.max(segmentWidth - 2, 1)}" height="10" fill="${entry.color}"/>`;
      offset += segmentWidth;
      return rect;
    })
    .join('');
  let legendX = 24;
  const legend = stats.languages
    .map((entry) => {
      const text = `${entry.name} ${((entry.lines / total) * 100).toFixed(1)}%`;
      const item = `<circle cx="${legendX + 5}" cy="226" r="5" fill="${entry.color}"/><text x="${legendX + 16}" y="230" class="small">${escape(text)}</text>`;
      legendX += text.length * 6.6 + 40;
      return item;
    })
    .join('');

  return frame(
    width,
    256,
    '#6366f1',
    `<text x="24" y="46" class="title">Staya</text>
<rect x="100" y="28" rx="11" width="${stats.version.length * 7.6 + 22}" height="22" fill="#6366f1" fill-opacity=".14"/>
<text x="111" y="43" class="small mono" style="fill:#6366f1;font-size:12px">v${escape(stats.version)}</text>
<text x="${width - 24}" y="44" class="small" text-anchor="end">updated ${stats.generatedAt.slice(0, 10)} · ${stats.commit.slice(0, 7)}</text>
${cellsSvg}
<clipPath id="bar"><rect x="24" y="190" width="${barWidth}" height="10" rx="5"/></clipPath>
<g clip-path="url(#bar)"><rect x="24" y="190" width="${barWidth}" height="10" fill="var(--track)"/>${segments}</g>
${legend}`,
  );
}

function largestFiles() {
  const width = 900;
  const max = stats.largest[0]?.lines ?? 1;
  const rows = stats.largest
    .map((file, index) => {
      const y = 76 + index * 40;
      const barWidth = ((width - 48 - 420) * file.lines) / max;
      return `<text x="24" y="${y}" class="small mono" style="fill:var(--fg);font-size:13px">${escape(truncate(file.path, 48))}</text>
<rect x="440" y="${y - 12}" width="${width - 48 - 420 - 70}" height="14" rx="7" fill="var(--track)"/>
<rect x="440" y="${y - 12}" width="${Math.max(barWidth - 70 * (file.lines / max), 8)}" height="14" rx="7" fill="#f43f5e" fill-opacity="${1 - index * 0.15}"/>
<text x="${width - 24}" y="${y}" class="small" text-anchor="end" style="fill:var(--fg);font-weight:600">${number(file.lines)}</text>`;
    })
    .join('\n');
  return frame(
    width,
    76 + stats.largest.length * 40,
    '#f43f5e',
    `<circle cx="28" cy="32" r="5" fill="#f43f5e"/><text x="42" y="36" class="label">LARGEST FILES · LINES</text>
${rows}`,
  );
}

const cards = {
  overview: overview(),
  tiles: tileGrid([
    tile({
      label: 'Rust code',
      value: number(stats.code.rust),
      sub: `core, proto, server · ${stats.crates.workspace} workspace crates`,
      accent: '#f97316',
    }),
    tile({
      label: 'Swift · Kotlin',
      value: `${number(stats.code.swift)} · ${number(stats.code.kotlin)}`,
      sub: 'lines in the iOS and Android apps',
      accent: '#f43f5e',
    }),
    tile({
      label: 'Tests',
      value: `${number(stats.tests.cases)} tests`,
      sub: `${number(stats.tests.files)} test files · ${number(stats.tests.lines)} lines`,
      accent: '#22c55e',
    }),
    tile({
      label: 'Plan progress',
      value: `${stats.plan.done} / ${stats.plan.total}`,
      sub: `tasks done in docs/PLAN.md`,
      accent: '#3b82f6',
    }),
    tile({
      label: 'Dependencies',
      value: `${number(stats.crates.external)} crates`,
      sub: `from crates.io · checked by cargo deny`,
      accent: '#8b5cf6',
    }),
    tile({
      label: 'Unsafe Rust',
      value: number(stats.unsafeBlocks),
      sub: 'unsafe blocks in our code',
      accent: '#ef4444',
    }),
    tile({
      label: 'Protocol',
      value: stats.protocolVersion,
      sub: `${stats.docs.markdown} Markdown files`,
      accent: '#a855f7',
    }),
    tile({
      label: 'Last 7 days',
      value: `${stats.commits.last7Days} commits`,
      sub: `last commit ${stats.commits.lastCommit.slice(0, 10)}`,
      accent: '#06b6d4',
    }),
    tile({
      label: 'TODO / FIXME',
      value: number(stats.todos),
      sub: `project age ${days(stats.ageDays)}`,
      accent: '#64748b',
    }),
  ]),
  'largest-files': largestFiles(),
};

mkdirSync(outputDir, { recursive: true });
for (const [name, svg] of Object.entries(cards)) writeFileSync(join(outputDir, `${name}.svg`), svg);
writeFileSync(join(outputDir, 'stats.json'), `${JSON.stringify(stats, null, 2)}\n`);
console.log(JSON.stringify({ ...stats, largest: stats.largest.slice(0, 1) }, null, 2));
