// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import sitemap from '@astrojs/sitemap';
import tailwindcss from '@tailwindcss/vite';
import starlightSidebarTopics from 'starlight-sidebar-topics'
import starlightLinksValidator from 'starlight-links-validator'

// https://astro.build/config
export default defineConfig({
  output: 'static',
  site: 'https://fairagro.github.io',
  base: '/sciwin/',
  integrations: [
    starlight({
      title: 'SciWIn Client',
      favicon: '/favicon.png',
      logo: {
        src: './src/assets/logo.svg',
        replacesTitle: true
      },
      customCss: [
        '@fontsource/fira-sans/400.css',
        '@fontsource/fira-sans/700.css',
        '@fontsource/fira-sans/900.css',
        '@fontsource/fira-sans/400-italic.css',
        '@fontsource/fira-sans/700-italic.css',
        '@fontsource/fira-sans/900-italic.css',
        './src/styles/global.css'
      ],
      social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/fairagro/sciwin' }],
      components: {
        Hero: './src/components/Hero.astro',
        PageFrame: './src/components/PageFrame.astro',
      },
      plugins: [
        starlightLinksValidator({
          errorOnRelativeLinks: false,
        }),
        starlightSidebarTopics([
          {
            label: 'Home',
            icon: 'puzzle',
            link: "/sciwin/"
          },
          {
            label: 'Documentation',
            icon: 'open-book',
            link: 'getting-started',
            items: [
              { label: 'Getting Started', autogenerate: { directory: 'getting-started' } },
              { label: 'Examples', autogenerate: { directory: 'examples' } },
              { label: 'Reference', autogenerate: { directory: 'reference' } },
            ]
          },
          {
            label: 'Download Latest Release',
            icon: 'download',
            link: 'https://github.com/fairagro/sciwin/releases/latest/',
          },
          {
            label: 'Report Issue',
            icon: 'add-document',
            link: 'https://github.com/fairagro/sciwin/issues/new',
          },
        ]),
      ]
    }),
    sitemap(),
  ],

  vite: {
    plugins: [tailwindcss()],
  },
});
