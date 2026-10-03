# The Spirit (third-party build)

The original build of **The Spirit** by Edan Kwan (https://github.com/edankwan/The-Spirit), MIT; see `license.txt`.
KLIF's Spirit skin runs it in an iframe and drives it from `app/ui/src/lib/fx/spirit/spirit.ts`.

Changes for KLIF: `js/index.js` exposes the settings object (`__spirit`), the intro progress (`__spiritJ`) and a
single frame step (`__spiritFrame`; with `#external=1` the page schedules no frames itself); `index.html` hides the
page's own logo, GUI and footer.

Bundled components:

- three.js r74 (`js/three.r74.min.js`), MIT, https://threejs.org/license
- dat.GUI (inside `js/index.js`), Apache-2.0, Google Data Arts Team
- normalize.css (`css/normalize.css`), MIT
- Oxygen font (`css/font/`), SIL Open Font License, see the file there
