// The documentation site, built from ../docs by scripts/sync.mjs. The pages
// under src/content/docs are generated and never committed: docs/ stays the
// one place the words live, readable on GitHub as they are.
//
// It is a project site under the org's Pages domain, with-love.lonkar.org,
// which every lonkar-org repository's Pages shares, so it lives at a path and
// not at a host of its own. The base is in scripts/sync.mjs too, for links.
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { BASE } from './scripts/sync.mjs';

const repo = 'https://github.com/lonkar-org/tmux-companion';

export default defineConfig({
  site: 'https://with-love.lonkar.org',
  base: BASE,
  integrations: [
    starlight({
      title: 'tmux-companion',
      description:
        'One binary behind your whole tmux config: the status bar, the pickers and your project sessions.',
      social: [{ icon: 'github', label: 'GitHub', href: repo }],
      editLink: { baseUrl: `${repo}/edit/main/docs/` },
      // tmux.conf has no grammar of its own in Shiki, and shell is close.
      expressiveCode: { shiki: { langAlias: { tmux: 'shellscript' } } },
      sidebar: [
        { label: 'Tutorial', items: [{ autogenerate: { directory: 'tutorial' } }] },
        { label: 'How-to guides', items: [{ autogenerate: { directory: 'how-to' } }] },
        {
          label: 'Reference',
          items: [
            'reference/cli',
            'reference/configuration',
            'reference/manual',
            'reference/requirements',
            { label: 'Example configs', items: [{ autogenerate: { directory: 'reference/examples' } }] },
          ],
        },
        { label: 'Explanation', items: [{ autogenerate: { directory: 'explanation' } }] },
      ],
    }),
  ],
});
