// The documentation site, built from ../docs by scripts/sync.mjs. The pages
// under src/content/docs are generated and never committed: docs/ stays the
// one place the words live, readable on GitHub as they are.
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

const repo = 'https://github.com/lonkar-org/tmux-companion';

export default defineConfig({
  site: 'https://tmux-companion.lonkar.org',
  integrations: [
    starlight({
      title: 'tmux-companion',
      description:
        'One binary behind your whole tmux config: the status bar, the pickers and your project sessions.',
      social: [{ icon: 'github', label: 'GitHub', href: repo }],
      editLink: { baseUrl: `${repo}/edit/main/docs/` },
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
