// @ts-check
import { readFileSync } from 'node:fs';
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import sitemap from '@astrojs/sitemap';
import starlightLinksValidator from 'starlight-links-validator'
import starlightVersions from 'starlight-versions'
import tailwindcss from '@tailwindcss/vite';
import mdx from '@astrojs/mdx';

// docs/versions.json maps an archived "X.Y" slug to the release tag its snapshot was
// generated from (e.g. "1.2": "v1.2.2"). It is kept in sync with the repo's release tags
// by `docs/scripts/sync-versions.mjs`, which `.github/workflows/docs.yml` runs before build.
const versionsBySlug = JSON.parse(readFileSync(new URL('./versions.json', import.meta.url), 'utf8'));
const versions = Object.keys(versionsBySlug)
  .sort((a, b) => b.localeCompare(a, undefined, { numeric: true }))
  .map((slug) => ({ slug, label: `v${slug}` }));

// https://astro.build/config
export default defineConfig({
  output: 'static',
  site: 'https://fairagro.github.io',
  base: '/sciwin/',

  integrations: [starlight({
    title: 'SciWIn',
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
      '@fontsource/fira-code/400.css',
      '@fontsource/fira-code/500.css',
      './src/styles/global.css'
    ],
    social: [
      { icon: 'github', label: 'GitHub', href: 'https://github.com/fairagro/sciwin' },
      { icon: 'download', label: 'Download Latest Release', href: 'https://github.com/fairagro/sciwin/releases/latest/' },
      { icon: 'add-document', label: 'Report Issue', href: 'https://github.com/fairagro/sciwin/issues/new' },
    ],
    components: {
      Hero: './src/components/Hero.astro',
      PageFrame: './src/components/PageFrame.astro',
    },
    sidebar: [
      { label: 'Getting Started', items: [{ autogenerate: { directory: 'getting-started' } }] },
      { label: 'Concepts', items: [{ autogenerate: { directory: 'concepts' } }] },
      { label: 'SciWIn-Studio', items: [{ autogenerate: { directory: 'sciwin-studio' } }] },
      { label: 'Examples', items: [{ autogenerate: { directory: 'examples' } }] },
      { label: 'Reference', items: [{ autogenerate: { directory: 'reference' } }] },
      { label: 'Development', items: [{ autogenerate: { directory: 'development' } }] },
    ],
    plugins: [
      starlightLinksValidator({
        errorOnRelativeLinks: false,
      }),
      starlightVersions({
        versions,
        current: { label: 'Latest' },
      }),
    ]
  }), sitemap(), mdx()],

  vite: {
    plugins: [tailwindcss()],
  },
});
