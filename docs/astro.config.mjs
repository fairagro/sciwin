// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import sitemap from '@astrojs/sitemap';
import starlightSidebarTopics from 'starlight-sidebar-topics'
import starlightLinksValidator from 'starlight-links-validator'
import tailwindcss from '@tailwindcss/vite';
import mdx from '@astrojs/mdx';
// https://astro.build/config
export default defineConfig({
  output: 'static',
  site: 'https://fairagro.github.io',
  base: '/sciwin/',

  integrations: [starlight({
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
            { label: 'Getting Started', items: [{ autogenerate: { directory: 'getting-started' } }] },
            { label: 'Examples', items: [{ autogenerate: { directory: 'examples' } }] },
            { label: 'Reference', items: [{ autogenerate: { directory: 'reference' } }] },
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
  }), sitemap(), mdx()],

  vite: {
    plugins: [tailwindcss()],
  },
});