// Local state of one open Tune drawer: preset drafts (keyed by node + preset id, never bound to the 2 Hz view
// model), which preset each System tab shows, open sections, and the drawer's confirm prompt. It lives as long
// as the drawer is mounted; closing with unsaved drafts asks first (ui.tuneCanClose).
import { getContext, setContext } from 'svelte';
import { specNumberProblem } from '../../model/presets';
import type { CommandView, PresetDetail, PresetInfo, PresetSpec, System, SystemId, ViewModel } from '../../model/types';
import { player } from '../../state/player.svelte';
import { ui } from '../../state/ui.svelte';
import { clearSecrets, REMOTE_SECRET_REFUSAL } from './secret';
import { attempt, cleanSpec, errorText, listNames, plain, presetIdProblem, QUIET, specKey } from './util';

export interface Draft {
  /** `<node>|<preset id>` for a stored preset, `new|<n>` for an unsaved new one. */
  key: string;
  node?: string;
  /** The preset id (a new preset: the id it will be saved as, editable). */
  id: string;
  isNew: boolean;
  /** A new preset becomes the active preset of this System on Apply. */
  selectFor?: SystemId;
  /** Masked secrets are resolved against this stored preset (Save as new). */
  secretsFrom?: string;
  /** The stored preset as loaded (null for a new one). */
  base: PresetDetail | null;
  /** `PresetDetail.specHash` of the stored preset when it was loaded (every field): sent as baseHash on Apply. */
  baseHash?: string;
  /** The spec as loaded (or the blank start of a new one). */
  baseSpec: PresetSpec;
  /** specKey(baseSpec). */
  baseJson: string;
  /** What the editor changes. */
  spec: PresetSpec;
  loading: boolean;
  loadError: string;
  saving: boolean;
  applyError: string;
  /** The last Apply was refused because the stored preset changed meanwhile ("changed on disk"). */
  stale: boolean;
  /** The command the edited spec would run (debounced preview). */
  preview: CommandView | null;
  previewError: string;
}

export interface ConfirmRequest {
  title: string;
  detail?: string;
  confirm: string;
  cancel?: string;
  danger?: boolean;
  resolve: (ok: boolean) => void;
}

/** Lists are edited in place by the editor, so a loaded spec always has them. */
function editable(spec: PresetSpec): PresetSpec {
  const s = plain(spec) ?? {};
  s.args ??= [];
  s.env ??= {};
  s.env_remove ??= [];
  return s;
}

/** Every preset of one machine (any kind): for id uniqueness. */
export function presetPool(vm: ViewModel, node: string | undefined): PresetInfo[] {
  return node ? (vm.nodes.find((n) => n.id === node)?.presets ?? []) : vm.presets;
}

export class TuneState {
  drafts = $state<Record<string, Draft>>({});
  /** Per System: the preset the Preset row and the Command editor show (a draft key `new|n` for a new one). */
  viewed = $state<Record<SystemId, string>>({});
  commandOpen = $state(false);
  recOpen = $state(false);
  nodesOpen = $state(false);
  webuiOpen = $state(false);
  confirmReq = $state<ConfirmRequest | null>(null);
  private seq = 0;

  /** The draft key of a preset id as viewed on a System. */
  keyOf(node: string | undefined, id: string): string {
    return id.startsWith('new|') ? id : `${node ?? ''}|${id}`;
  }

  /** The preset a System's tab shows: the one picked in the Preset row, else its active preset ('' none). */
  viewedId(system: System): string {
    const v = this.viewed[system.id];
    if (v && (!v.startsWith('new|') || this.drafts[v])) return v;
    return system.preset ?? '';
  }

  view(system: System, id: string) {
    this.viewed[system.id] = id;
  }

  draft(node: string | undefined, id: string): Draft | undefined {
    return id ? this.drafts[this.keyOf(node, id)] : undefined;
  }

  /** Make sure the stored preset's draft exists (loads it once). Call from effects or handlers, never from a derived. */
  ensure(node: string | undefined, id: string) {
    if (!id || id.startsWith('new|')) return;
    const key = this.keyOf(node, id);
    if (this.drafts[key]) return;
    this.drafts[key] = {
      key,
      node,
      id,
      isNew: false,
      base: null,
      baseSpec: {},
      baseJson: '',
      spec: {},
      loading: true,
      loadError: '',
      saving: false,
      applyError: '',
      stale: false,
      preview: null,
      previewError: '',
    };
    void this.load(key);
  }

  /** (Re)load a stored preset into its draft; the edits are replaced. */
  async load(key: string) {
    const d = this.drafts[key];
    if (!d || d.isNew) return;
    d.loading = true;
    let detail: PresetDetail | null = null;
    let error = '';
    try {
      detail = await player.config.presetGet(d.id, d.node);
      if (!detail) error = `Preset "${d.id}" is not in klif.toml${d.node ? ' on that node' : ''}.`;
    } catch (e) {
      error = errorText(e) || 'The preset could not be read.';
    }
    const cur = this.drafts[key];
    if (!cur) return;
    cur.loading = false;
    cur.loadError = error;
    if (!detail) return;
    cur.base = detail;
    cur.baseHash = detail.specHash || undefined;
    cur.baseSpec = editable(detail.spec);
    cur.baseJson = specKey(cur.baseSpec);
    cur.spec = editable(detail.spec);
    cur.applyError = '';
    cur.stale = false;
  }

  /** A new, unsaved preset (Blank / New preset). Returns its draft key. */
  createNew(node: string | undefined, id: string, spec: PresetSpec, opts: { selectFor?: SystemId; secretsFrom?: string } = {}): string {
    const key = `new|${++this.seq}`;
    const start = editable(spec);
    this.drafts[key] = {
      key,
      node,
      id,
      isNew: true,
      selectFor: opts.selectFor,
      secretsFrom: opts.secretsFrom,
      base: null,
      baseSpec: start,
      baseJson: specKey(start),
      spec: editable(spec),
      loading: false,
      loadError: '',
      saving: false,
      applyError: '',
      stale: false,
      preview: null,
      previewError: '',
    };
    return key;
  }

  /** Unsaved edits worth a confirm before they are thrown away. */
  isDirty(d: Draft | undefined): boolean {
    if (!d || d.loading || d.loadError) return false;
    return specKey(d.spec) !== d.baseJson;
  }

  /** Apply has something to write: a new preset, or edits. */
  needsApply(d: Draft | undefined): boolean {
    if (!d || d.loading || d.loadError) return false;
    return d.isNew || this.isDirty(d);
  }

  dirtyDrafts(): Draft[] {
    return Object.values(this.drafts).filter((d) => this.isDirty(d));
  }

  draftName(d: Draft): string {
    return d.isNew ? `the new preset "${d.id}"` : (d.base?.info.name ?? d.id);
  }

  /** Throw the edits away (back to what was loaded). */
  revert(key: string) {
    const d = this.drafts[key];
    if (!d) return;
    d.spec = editable(d.baseSpec);
    d.applyError = '';
  }

  drop(key: string) {
    delete this.drafts[key];
  }

  /**
   * Write the draft (SavePreset with baseHash; node for a remote System). Resolves true when it was saved; the
   * error sentence stays on the draft (applyError) for the Command section and the footer.
   */
  async apply(vm: ViewModel, system: System, key: string): Promise<boolean> {
    const d = this.drafts[key];
    if (!d || d.saving) return false;
    d.applyError = '';
    const spec = cleanSpec(plain(d.spec));
    const bad = specNumberProblem(spec);
    if (bad) {
      d.applyError = bad;
      return false;
    }
    if (system.node) {
      const clear = clearSecrets(spec);
      if (clear.length) {
        d.applyError = `${REMOTE_SECRET_REFUSAL} (${listNames(clear)}).`;
        return false;
      }
    }
    if (d.isNew) {
      const problem = presetIdProblem(d.id, presetPool(vm, d.node).map((p) => p.id));
      if (problem) {
        d.applyError = problem;
        return false;
      }
    }
    d.saving = true;
    const err = await attempt(() =>
      player.actions.savePreset(d.id, spec, {
        baseHash: d.isNew ? undefined : d.baseHash || undefined,
        node: system.node,
        selectFor: d.isNew ? d.selectFor : undefined,
        secretsFrom: d.secretsFrom,
        ...QUIET,
      }),
    );
    const cur = this.drafts[key];
    if (!cur) return !err;
    cur.saving = false;
    if (err) {
      cur.applyError = err;
      cur.stale = /changed on disk/i.test(err);
      return false;
    }
    if (cur.isNew) {
      const id = cur.id;
      delete this.drafts[key];
      if (this.viewed[system.id] === key) this.viewed[system.id] = id;
      this.ensure(system.node, id);
    } else {
      await this.load(key);
    }
    return true;
  }

  /** Ask in the drawer (a small dialog inside the sheet). Resolves true on confirm. */
  confirm(req: Omit<ConfirmRequest, 'resolve'>): Promise<boolean> {
    this.confirmReq?.resolve(false);
    return new Promise((resolve) => {
      this.confirmReq = { ...req, resolve };
    });
  }

  answer(ok: boolean) {
    const r = this.confirmReq;
    this.confirmReq = null;
    r?.resolve(ok);
  }

  /** ui.tuneCanClose: Escape / Close / the scrim. False keeps the drawer open (it asks about unsaved edits first). */
  canClose(): boolean {
    if (this.confirmReq) {
      this.answer(false);
      return false;
    }
    const dirty = this.dirtyDrafts();
    if (!dirty.length) return true;
    void this.confirm({
      title: 'Discard unsaved changes?',
      detail: `Edits to ${listNames(dirty.map((d) => this.draftName(d)))} are not applied yet.`,
      confirm: 'Discard',
      cancel: 'Keep editing',
      danger: true,
    }).then((ok) => {
      if (!ok) return;
      this.drafts = {};
      ui.closeTune();
    });
    return false;
  }
}

// A string key (not a Symbol): it survives a hot reload of this module during development.
const KEY = 'klif-tune-state';

export function setTune(t: TuneState): TuneState {
  return setContext(KEY, t);
}

export function getTune(): TuneState {
  return getContext<TuneState>(KEY);
}
