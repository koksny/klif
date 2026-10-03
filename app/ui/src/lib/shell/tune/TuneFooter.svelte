<script lang="ts">
  // The drawer footer of a System tab. The primary button names exactly what it does, composed from the steps
  // it takes: Apply (the shown preset has unapplied edits), Use (the tab shows another preset than the active
  // one), then Launch / Restart. With conflicts the button is the question: "Stop <labels> & launch" sends
  // stopOthers. External servers get a note instead of Launch; a running session whose command differs from the
  // current one offers "Restart to apply".
  import { untrack } from 'svelte';
  import { specNumberProblem } from '../../model/presets';
  import { isHeld, isPendingLaunch, launchCtl, nodeName } from '../../model/systems';
  import type { System, ViewModel } from '../../model/types';
  import { player } from '../../state/player.svelte';
  import { getTune } from './state.svelte';
  import { attempt, listNames, QUIET } from './util';

  interface Props {
    vm: ViewModel;
    system: System;
    viewedId: string;
  }
  let { vm, system, viewedId }: Props = $props();
  const t = getTune();

  type Step = () => Promise<string>;
  interface Btn {
    label: string;
    steps?: Step[];
    disabled?: boolean;
    title?: string;
  }
  interface Plan {
    note?: string;
    warn?: boolean;
    primary?: Btn;
    secondary?: Btn[];
  }

  let busy = $state(false);
  let err = $state('');

  const draft = $derived(viewedId ? t.draft(system.node, viewedId) : undefined);
  const isNewView = $derived(viewedId.startsWith('new|'));
  const applyStep = $derived(t.needsApply(draft));
  // A new preset that does not select itself (the System already has one) still needs Use after Apply.
  const useStep = $derived(
    !!viewedId && (isNewView ? !!draft && draft.selectFor !== system.id : viewedId !== system.preset),
  );
  const previewErrors = $derived((draft?.preview?.issues ?? []).some((i) => i.level === 'error'));
  /** A port / ctx the core could not even read: nothing can be applied until it is fixed. */
  const numberProblem = $derived(applyStep && draft ? specNumberProblem(draft.spec) : '');
  const viewedInfo = $derived(
    isNewView ? undefined : (system.node ? vm.nodes.find((n) => n.id === system.node)?.presets : vm.presets)?.find((p) => p.id === viewedId),
  );
  const ghost = $derived(!system.node && !system.editable);
  const status = $derived(system.status);

  // An error belongs to the situation it happened in: it goes when that changes (not while steps still run).
  const scope = $derived(`${viewedId}|${applyStep}|${useStep}|${status}`);
  $effect(() => {
    void scope;
    untrack(() => {
      if (!busy) err = '';
    });
  });
  const differs = $derived(
    !!system.session?.command?.hash && !!system.command?.hash && system.session.command.hash !== system.command.hash,
  );

  // ---- steps ----
  const doApply: Step = async () => {
    const d = draft;
    if (!d) return '';
    const id = d.id;
    const ok = await t.apply(vm, system, d.key);
    if (!ok) return d.applyError || 'Apply failed.';
    applied = id;
    return '';
  };
  // The id the Use step needs is known only after Apply (a new preset's id).
  let applied = '';
  const doUse: Step = () => {
    const id = isNewView ? applied || draft?.id || '' : viewedId;
    return attempt(() => player.actions.usePreset(system.id, id, QUIET));
  };
  const doLaunch = (stopOthers: boolean): Step => () => attempt(() => player.actions.launch(system.id, stopOthers ? { stopOthers: true, ...QUIET } : QUIET));
  const doRestart: Step = () => attempt(() => player.actions.restart(system.id, QUIET));
  const doStop: Step = () => attempt(() => player.actions.stop(system.id, QUIET));
  const doDismiss: Step = () => attempt(() => player.actions.dismiss(system.id, QUIET));
  const doReload: Step = async () => {
    const d = draft;
    if (!d) return '';
    if (t.isDirty(d)) {
      const ok = await t.confirm({
        title: 'Reload from klif.toml?',
        detail: 'The stored preset changed meanwhile. Reloading drops your edits.',
        confirm: 'Reload',
        danger: true,
      });
      if (!ok) return '';
    }
    await t.load(d.key);
    err = '';
    return '';
  };

  /** "Use & launch", "Apply, use & restart"; with Systems to stop first: "Use, stop System 1 & launch". */
  function composed(verb: 'launch' | 'restart', stop: string[] = []): string {
    const pre = applyStep && useStep ? 'Apply, use' : applyStep ? 'Apply' : 'Use';
    return `${pre}${stop.length ? `, stop ${listNames(stop)}` : ''} & ${verb}`;
  }

  const plan = $derived.by((): Plan => {
    const s = system;
    const stopBtn: Btn = { label: `Stop ${s.label}`, steps: [doStop] };
    const fixSteps: Step[] = [...(applyStep ? [doApply] : []), ...(useStep ? [doUse] : [])];
    const fixLabel = applyStep && useStep ? 'Apply & use' : applyStep ? 'Apply' : 'Use';

    if (s.status === 'unreachable') return { note: s.reason ?? 'Its node is not reachable right now.', warn: true };
    if (ghost) {
      if (s.status === 'fault') return { note: 'Not in klif.toml any more.', primary: { label: 'Dismiss', steps: [doDismiss] } };
      return { note: 'Not in klif.toml any more: only Stop is offered.', primary: isHeld(s) ? stopBtn : undefined };
    }
    if (!s.controllable) {
      return { note: `View only: ${nodeName(vm, s) ?? 'that node'} does not allow launching from here.` };
    }
    if (s.external) {
      return {
        note: numberProblem || 'External server: KLIF watches it and never starts or stops it.',
        warn: !!numberProblem,
        primary: applyStep ? { label: fixLabel, steps: fixSteps, disabled: !!numberProblem } : useStep ? { label: 'Use', steps: [doUse] } : undefined,
      };
    }
    if (s.status === 'stopping') return { note: 'Stopping…' };

    if (isPendingLaunch(s)) {
      // "Stop X & launch" in progress: it waits for X to stop; Stop cancels it.
      return { note: s.reason ?? 'Waiting to launch.', secondary: [{ label: 'Cancel launch', steps: [doStop] }] };
    }
    if (s.status === 'starting' || s.status === 'online' || s.status === 'busy') {
      const sec = [stopBtn];
      const interrupt = s.status === 'busy' ? ` ${s.label} is working right now: restarting interrupts it.` : '';
      if (applyStep || useStep) {
        if (numberProblem) return { note: numberProblem, warn: true, primary: { label: composed('restart'), disabled: true }, secondary: sec };
        if (applyStep && previewErrors) {
          return { note: 'The edited command has errors: Apply saves it, Restart waits until they are fixed.', warn: true, primary: { label: fixLabel, steps: fixSteps }, secondary: sec };
        }
        return { note: `Restarts with the shown preset.${interrupt}`, warn: !!interrupt, primary: { label: composed('restart'), steps: [...fixSteps, doRestart] }, secondary: sec };
      }
      if (differs) {
        const blocked = s.availability !== 'ready';
        return {
          note: blocked ? (s.reason ?? 'The current preset cannot launch.') : `Differs from the running session.${interrupt}`,
          warn: true,
          primary: { label: 'Restart to apply', steps: [doRestart], disabled: blocked },
          secondary: sec,
        };
      }
      return { note: 'Running. Changes apply on the next launch.', secondary: sec };
    }

    // offline / invalid / not-set / fault
    const sec: Btn[] = s.status === 'fault' ? [{ label: 'Dismiss', steps: [doDismiss] }] : [];
    if (!viewedId) {
      return { note: 'Pick a preset, take a recommendation, or start a blank one.', primary: { label: `Launch ${s.label}`, disabled: true }, secondary: sec };
    }
    if (applyStep || useStep) {
      if (numberProblem) return { note: numberProblem, warn: true, primary: { label: composed('launch'), disabled: true }, secondary: sec };
      if (applyStep && previewErrors) {
        return { note: 'The edited command has errors: Apply saves it, Launch waits until they are fixed.', warn: true, primary: { label: fixLabel, steps: fixSteps }, secondary: sec };
      }
      if (!applyStep && viewedInfo && viewedInfo.availability !== 'ready') {
        return { note: viewedInfo.reason ?? 'That preset cannot launch yet.', warn: true, primary: { label: 'Use', steps: [doUse] }, secondary: sec };
      }
      // The launch is still the question when other Systems hold what it needs (SPEC 16.8): the label names them
      // and the launch sends stopOthers. Only the names count here: enabled/blocked judge the ACTIVE preset, and
      // this launches the shown one. `conflicts` is computed for the active preset too, so a conflict that only
      // the shown preset has is refused once; the footer then relabels from the new System state.
      const ctl = launchCtl(vm, s);
      const stop = ctl.stopOthers;
      return {
        note: stop ? `${listNames(ctl.names)} ${ctl.names.length === 1 ? 'has' : 'have'} to stop first.` : 'Launches with the shown preset.',
        warn: stop,
        primary: { label: composed('launch', stop ? ctl.names : []), steps: [...fixSteps, doLaunch(stop)] },
        secondary: sec,
      };
    }
    const ctl = launchCtl(vm, s);
    if (!ctl.enabled) {
      return { note: ctl.blocked, warn: true, primary: { label: `Launch ${s.label}`, disabled: true }, secondary: sec };
    }
    if (ctl.stopOthers) {
      return {
        note: s.reason ?? `${listNames(ctl.names)} ${ctl.names.length === 1 ? 'has' : 'have'} to stop first.`,
        warn: true,
        primary: { label: ctl.text, steps: [doLaunch(true)] },
        secondary: sec,
      };
    }
    return { note: 'Changes apply on the next launch.', primary: { label: `Launch ${s.label}`, steps: [doLaunch(false)] }, secondary: sec };
  });

  /** "changed on disk": the footer offers Reload next to whatever it offers. */
  const shown = $derived.by((): Plan => {
    if (!draft?.stale) return plan;
    return { ...plan, secondary: [{ label: 'Reload', steps: [doReload], title: 'Read the stored preset again (drops your edits)' }, ...(plan.secondary ?? [])] };
  });

  async function run(b: Btn) {
    if (!b.steps || busy) return;
    busy = true;
    err = '';
    applied = '';
    for (const step of b.steps) {
      const e = await step();
      if (e) {
        err = e;
        break;
      }
    }
    busy = false;
  }
</script>

<footer>
  {#if shown.note}<p class:warn={shown.warn}>{shown.note}</p>{/if}
  {#if err}<p class="err" role="alert">{err}</p>{/if}
  {#if shown.primary || shown.secondary?.length}
    <div class="btns">
      {#each shown.secondary ?? [] as b (b.label)}
        <button type="button" disabled={busy || b.disabled} title={b.title ?? ''} onclick={() => void run(b)}>{b.label}</button>
      {/each}
      {#if shown.primary}
        <button type="button" class="primary" disabled={busy || shown.primary.disabled || !shown.primary.steps} title={shown.primary.title ?? ''} onclick={() => shown.primary && void run(shown.primary)}>
          {shown.primary.label}
        </button>
      {/if}
    </div>
  {/if}
</footer>

<style>
  footer {
    padding: 12px 18px 16px;
    border-top: 1px solid var(--k-line, #2e2e2e);
    display: grid;
    gap: 10px;
    flex: none;
  }
  footer p {
    margin: 0;
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
  }
  footer p.warn {
    color: var(--k-warn, #f2a33a);
  }
  footer p.err {
    color: var(--k-danger, #e05a5a);
  }
  .btns {
    display: flex;
    gap: 8px;
  }
  .btns .primary {
    flex: 1;
  }
</style>
