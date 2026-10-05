import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve, sep } from 'node:path';
import { defineConfig, type Plugin } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Fonts ship as woff2 only: every WebView2 and browser KLIF runs in reads it, so the .woff fallbacks that the
// @fontsource stylesheets list (and that Vite would copy into the build) are cut from the CSS before Vite sees it.
function woff2Only(): Plugin {
  return {
    name: 'klif-woff2-only',
    enforce: 'pre',
    transform(code, id) {
      if (!/@fontsource[^?]*\.css(\?|$)/.test(id.replace(/\\/g, '/'))) return null;
      const out = code.replace(/,\s*url\([^)]*\.woff\)\s*format\((['"])woff\1\)/g, '');
      return out === code ? null : { code: out, map: null };
    },
  };
}

// public/ is copied into the build, except public/private/**: local-only art that must never ship. (Vite's own
// publicDir copy has no exclude, so a build emits public/ itself, skipping that folder; the dev server serves
// public/ as usual.) The build fails on any font that is not woff2, from public/ or from the bundle: the build is
// embedded into klif.exe, and fonts ship as woff2 only.
const NOT_WOFF2 = /\.(otf|ttf|woff|eot)$/i;

function publicWithoutPrivate(publicDir: string): Plugin {
  return {
    name: 'klif-public-without-private',
    apply: 'build',
    generateBundle(_options, bundle) {
      for (const fileName of Object.keys(bundle)) {
        if (NOT_WOFF2.test(fileName)) this.error(`${fileName}: fonts ship as woff2 only.`);
      }
      if (!existsSync(publicDir)) return;
      const walk = (dir: string) => {
        for (const name of readdirSync(dir)) {
          const full = join(dir, name);
          const rel = relative(publicDir, full).split(sep).join('/');
          if (rel === 'private' || rel.startsWith('private/')) continue;
          if (statSync(full).isDirectory()) walk(full);
          else if (NOT_WOFF2.test(rel)) this.error(`public/${rel}: fonts ship as woff2 only.`);
          else this.emitFile({ type: 'asset', fileName: rel, source: readFileSync(full) });
        }
      };
      walk(publicDir);
    },
  };
}

// The dev-server port is registered in .studio/devserver.json. Never change it here alone.
export default defineConfig(({ command }) => ({
  plugins: [woff2Only(), svelte(), publicWithoutPrivate(resolve(__dirname, 'public'))],
  publicDir: command === 'build' ? false : 'public',
  server: {
    host: '127.0.0.1',
    port: 5193,
    strictPort: true,
  },
  preview: {
    host: '127.0.0.1',
    port: 5193,
    strictPort: true,
  },
  build: {
    target: 'es2022',
    // Two pages: the window (index.html) and klif-webui (webui.html), which the engine serves on the LAN. They
    // share the skins' token and font chunks.
    rolldownOptions: {
      input: { main: resolve(__dirname, 'index.html'), webui: resolve(__dirname, 'webui.html') },
    },
  },
}));
