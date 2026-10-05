<script lang="ts">
  // What a WebGL skin shows where its picture would be when this window cannot draw it: what is missing, that the
  // rest of KLIF works, and how to switch. In the active skin's tokens; no motion. Sits in the picture's own
  // positioned box, centred on it or on `at` (box px) when the picture's centre is elsewhere, but never past the
  // box's sides.
  interface Props {
    /** The skin, e.g. "Spirit". */
    skin: string;
    /** What it needs: "WebGL 2", "WebGL with float textures". */
    needs: string;
    /** The browser's own sentence, when there is one. */
    detail?: string;
    /** Replaces "This window does not have it": when the cause is not certain. */
    reason?: string;
    /** Centre of the card in box px; the box's centre when left out. */
    at?: { x: number; y: number } | null;
    /** The 960x640 panel, read from about a metre: larger type. */
    panel?: boolean;
  }
  let { skin, needs, detail = '', reason = '', at = null, panel = false }: Props = $props();

  const place = $derived(at ? `--x:${at.x}px; top:${at.y}px` : '');
</script>

<div class="nogl" class:panel role="status" style={place}>
  <b>{skin} needs {needs}</b>
  <p>{reason || `This window does not have it${detail ? ` (${detail})` : ''}.`} Everything else in KLIF works.</p>
  <p>Switch skins with F2 or the tray's Skin menu: Cliff, Silicon, Instrument and Phosphor draw without WebGL.</p>
</div>

<style>
  .nogl {
    --w: 420px;
    --m: 16px;
    --half: min(calc(var(--w) / 2), calc(50% - var(--m)));
    position: absolute;
    z-index: 1;
    left: clamp(calc(var(--half) + var(--m)), var(--x, 50%), calc(100% - var(--half) - var(--m)));
    top: 50%;
    transform: translate(-50%, -50%);
    width: calc(2 * var(--half));
    box-sizing: border-box;
    padding: 16px 18px;
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    background: var(--k-surface, #151515);
    color: var(--k-ink, #eee);
    font-family: var(--k-font-ui, system-ui, sans-serif);
    pointer-events: none;
  }
  b {
    display: block;
    margin-bottom: 8px;
    font-size: 15px;
    font-weight: 600;
    line-height: 1.3;
  }
  p {
    margin: 0;
    font-size: 13px;
    line-height: 1.45;
    color: var(--k-muted, #999);
  }
  p + p {
    margin-top: 6px;
  }
  .panel {
    --w: 540px;
    --m: 12px;
    padding: 14px 16px;
  }
  .panel b {
    font-size: 22px;
  }
  .panel p {
    font-size: 16px;
    line-height: 1.4;
  }
</style>
