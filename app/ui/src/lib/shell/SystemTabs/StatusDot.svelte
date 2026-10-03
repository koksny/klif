<script lang="ts">
  // The System status dot (SPEC 13.1, authored by the orchestrator). Colours come from the active skin's
  // --k-* tokens (published on :root by the shell). 6 px circle, 6 px gap before the label.
  //   online      solid accent                       busy      solid accent, opacity pulse 1.0-0.35 over 1.2 s
  //   starting / stopping  1.5 px accent ring, opacity pulse 0.4-1 over 0.8 s
  //   offline     1.5 px muted ring                  fault     solid warn
  //   unreachable 1.5 px dashed muted ring           not-set / invalid  no dot (the label carries it)
  import { STATUS_TEXT } from '../../model/systems';
  import type { SystemStatus } from '../../model/types';

  interface Props {
    status: SystemStatus;
    reason?: string;
    /** Diameter in px (default 6). */
    size?: number;
  }
  let { status, reason, size = 6 }: Props = $props();

  const visible = $derived(status !== 'not-set' && status !== 'invalid');
  const title = $derived(reason ? `${STATUS_TEXT[status]}: ${reason}` : STATUS_TEXT[status]);
</script>

{#if visible}
  <span class="dot {status}" style:--d="{size}px" {title} role="img" aria-label={title}>
    {#if status === 'unreachable'}
      <svg viewBox="0 0 6 6" width="100%" height="100%" aria-hidden="true">
        <circle cx="3" cy="3" r="2.25" fill="none" stroke="currentColor" stroke-width="1.5" stroke-dasharray="2 2" />
      </svg>
    {/if}
  </span>
{/if}

<style>
  .dot {
    display: inline-block;
    flex: none;
    width: var(--d, 6px);
    height: var(--d, 6px);
    margin-right: var(--d, 6px);
    border-radius: 50%;
    align-self: center;
    vertical-align: middle;
    box-sizing: border-box;
  }
  .online {
    background: var(--k-accent);
  }
  .busy {
    background: var(--k-accent);
    animation: busy 1.2s ease-in-out infinite;
  }
  .starting,
  .stopping {
    border: 1.5px solid var(--k-accent);
    animation: boot 0.8s ease-in-out infinite;
  }
  .offline {
    border: 1.5px solid var(--k-muted);
  }
  .fault {
    background: var(--k-warn);
  }
  .unreachable {
    color: var(--k-muted);
  }
  .unreachable svg {
    display: block;
  }
  @keyframes busy {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }
  @keyframes boot {
    0%,
    100% {
      opacity: 0.4;
    }
    50% {
      opacity: 1;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .busy {
      animation: none;
      outline: 1px solid var(--k-accent);
      outline-offset: 2px;
    }
    .starting,
    .stopping {
      animation: none;
    }
  }
</style>
