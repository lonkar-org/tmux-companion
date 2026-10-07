// Turn ../docs into Starlight pages under src/content/docs.
//
// usage: node scripts/sync.mjs        (npm run sync; `build` and `dev` run it)
//   env DOCS_REF   the git ref links to files outside the site point at,
//                  `main` by default; the release workflow sets the tag
//
// docs/ is written to be read on GitHub, so it has no frontmatter and links
// to files by relative path. Each page's H1 becomes its `title:`, a link to
// another published page becomes that page's route, a link to anything else
// in the repository becomes a GitHub link at DOCS_REF, and the man page is
// rendered with mandoc. Only the user docs are published: tutorial, how-to,
// reference and explanation, the manual and the example configs. dev/ and
// backlog/ stay in the repository.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../..');
const docs = path.join(root, 'docs');
const out = path.resolve(here, '../src/content/docs');
const ref = process.env.DOCS_REF || 'main';
const blob = `https://github.com/lonkar-org/tmux-companion/blob/${ref}/`;

const SECTIONS = ['tutorial', 'how-to', 'reference', 'explanation'];

/** Where the site sits under with-love.lonkar.org. Astro prefixes its own
 * links with it; a link written in Markdown gets it here. */
export const BASE = '/tmux-companion';
// The example files, each a page of its own: the page name, the title and
// the language its code block is highlighted as.
const EXAMPLES = {
  'config.example.toml': ['config-toml', 'config.toml, every setting', 'toml'],
  'tmux.conf.starter.example': ['tmux-conf-starter', 'tmux.conf, starter', 'sh'],
  'tmux.conf.example': ['tmux-conf', 'tmux.conf', 'sh'],
  'tmux.conf.full.example': ['tmux-conf-full', 'tmux.conf, every binding', 'sh'],
};

/** The site route for a repository path, or null when it isn't published. */
export function route(repoPath) {
  const p = repoPath.replace(/\\/g, '/');
  if (p === 'README.md' || p === 'docs/README.md') return `${BASE}/`;
  const m = p.match(/^docs\/([^/]+)\/(.+)\.md$/);
  if (m && SECTIONS.includes(m[1])) return `${BASE}/${m[1]}/${m[2].toLowerCase()}/`;
  const ex = p.match(/^docs\/([^/]+)$/);
  if (ex && EXAMPLES[ex[1]]) return `${BASE}/reference/examples/${EXAMPLES[ex[1]][0]}/`;
  if (p === 'docs/tmux-companion.1') return `${BASE}/reference/manual/`;
  return null;
}

/** Rewrite the link targets in Markdown written at `from` (a repo path). */
export function relink(markdown, from) {
  const dir = path.posix.dirname(from);
  const fix = (target) => {
    if (/^([a-z]+:|#|\/)/i.test(target)) return target;
    const [file, hash = ''] = target.split('#');
    const resolved = path.posix.normalize(path.posix.join(dir, file));
    const anchor = hash ? `#${hash}` : '';
    const page = route(resolved);
    return page ? page + anchor : blob + resolved + anchor;
  };
  // Inline links and images, and reference-style definitions.
  return markdown
    .replace(/(\]\()([^)\s]+)(\))/g, (_, a, t, b) => a + fix(t) + b)
    .replace(/^(\[[^\]]+\]:\s+)(\S+)/gm, (_, a, t) => a + fix(t));
}

/** The H1 as the title, taken off the body. */
export function titled(markdown) {
  const m = markdown.match(/^# (.+)\n+/);
  if (!m) return { title: null, body: markdown };
  return { title: m[1].trim(), body: markdown.slice(m[0].length) };
}

function frontmatter(fields) {
  const lines = Object.entries(fields).map(([k, v]) => `${k}: ${JSON.stringify(v)}`);
  return `---\n${lines.join('\n')}\n---\n\n`;
}

function write(rel, text) {
  const file = path.join(out, rel);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text);
}

function sync() {
  fs.rmSync(out, { recursive: true, force: true });

  for (const section of SECTIONS) {
    for (const name of fs.readdirSync(path.join(docs, section)).sort()) {
      if (!name.endsWith('.md')) continue;
      const from = `docs/${section}/${name}`;
      const { title, body } = titled(fs.readFileSync(path.join(root, from), 'utf8'));
      if (!title) throw new Error(`${from} has no # title on its first line`);
      write(`${section}/${name}`, frontmatter({ title }) + relink(body, from));
    }
  }

  // The landing page is the README, less its H1 and badges: the site has
  // its own title, and the badges are for the repository page.
  const readme = fs.readFileSync(path.join(root, 'README.md'), 'utf8');
  const body = readme.replace(/^# .+\n+/, '').replace(/^(\[!\[.*\n)+\n*/, '');
  write('index.md', frontmatter({ title: 'tmux-companion', editUrl: false }) + relink(body, 'README.md'));

  for (const [name, [page, title, lang]] of Object.entries(EXAMPLES)) {
    const text = fs.readFileSync(path.join(docs, name), 'utf8');
    const intro = `The file as it ships, [\`docs/${name}\`](${blob}docs/${name}).\n\n`;
    write(
      `reference/examples/${page}.md`,
      frontmatter({ title, editUrl: `https://github.com/lonkar-org/tmux-companion/edit/main/docs/${name}` }) +
        intro +
        '````' + lang + '\n' + text.replace(/\n$/, '') + '\n````\n',
    );
  }

  // mandoc's HTML is one raw block in Markdown only while it has no blank
  // line in it, since a blank line ends an HTML block; an empty comment keeps
  // each one, and inside <pre> the line still reads as empty.
  const manual = execFileSync('mandoc', ['-T', 'html', '-O', 'fragment', path.join(docs, 'tmux-companion.1')], {
    encoding: 'utf8',
  });
  // Its section headings become Markdown ones, so the page gets an "On this
  // page" list; the id each had stays on an empty anchor, since the manual's
  // own links point at it.
  const html = manual
    .replace(/^\s*$/gm, '<!---->')
    .replace(
      /<h([12]) class="S[hs]" id="([^"]+)"><a class="permalink" href="#[^"]+">([\s\S]*?)<\/a><\/h\1>/g,
      (_, level, id, text) =>
        `<span id="${id}"></span>\n\n${'#'.repeat(Number(level) + 1)} ${text.replace(/\s+/g, ' ').trim()}\n\n`,
    );
  write(
    'reference/manual.md',
    frontmatter({
      title: 'Manual page',
      description: 'tmux-companion(1), rendered from docs/tmux-companion.1.',
      editUrl: 'https://github.com/lonkar-org/tmux-companion/edit/main/docs/tmux-companion.1',
    }) +
      `What \`man tmux-companion\` shows, rendered from [\`docs/tmux-companion.1\`](${blob}docs/tmux-companion.1).\n\n<div class="manual">\n${html}\n</div>\n`,
  );
}

if (import.meta.url === `file://${process.argv[1]}`) {
  sync();
  console.log(`synced docs/ into ${path.relative(process.cwd(), out)} with links at ${ref}`);
}
