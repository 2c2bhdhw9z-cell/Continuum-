/**
 * Detail sheet: metadata, launch button, and the save-state list.
 *
 * The state list is the third use of `VirtualScroller`, and the one that justifies
 * it least obviously — until a game has 200 states. A single pooled set of rows
 * (about six, for a 208 px tall list) handles any history length, and opening the
 * sheet for a game with 200 states costs the same as one with none.
 */

import { VirtualScroller } from './virtual-scroller.js';
import { artFor, glyphFor, metaLineFor } from '../ui/art.js';
import { entryById, toggleFavorite } from '../data/catalog.js';
import { getSystem } from '../data/systems.js';
import { getCorePreference, setCorePreference } from '../data/core-prefs.js';
import { formatAge, listFor, autoStateFor } from '../data/save-states.js';

/** Matches `.state-row { height: 52px }`. */
const ROW_HEIGHT = 52;

export class DetailSheet {
  /**
   * @param {object} opts
   * @param {(entryId: string) => void} opts.onLaunch
   * @param {(entryId: string, slot: number) => void} opts.onLoadState
   * @param {() => void} opts.onDataChanged
   * @param {{ wake: (frames?: number) => void }} opts.scheduler
   */
  constructor({
    onLaunch,
    onLoadState,
    onDataChanged,
    onRemoveRom,
    onClearResume,
    scheduler,
    coresForSystem = () => [],
  }) {
    this.onClearResume = onClearResume;
    this.onLaunch = onLaunch;
    this.onLoadState = onLoadState;
    this.onDataChanged = onDataChanged;
    this.onRemoveRom = onRemoveRom;
    this.scheduler = scheduler;
    this.coresForSystem = coresForSystem;

    this.root = document.getElementById('detail-sheet');
    this.artEl = document.getElementById('detail-art');
    this.titleEl = document.getElementById('detail-title');
    this.metaEl = document.getElementById('detail-meta');
    this.blurbEl = document.getElementById('detail-blurb');
    this.playBtn = document.getElementById('detail-play');
    this.favBtn = document.getElementById('detail-favorite');
    this.removeBtn = document.getElementById('detail-remove');
    this.statesEl = document.getElementById('detail-states');
    this.statesCountEl = document.getElementById('detail-states-count');
    this.coreRow = document.getElementById('detail-core-row');
    this.coreSelect = document.getElementById('detail-core');
    this.coreNote = document.getElementById('detail-core-note');
    this.resumeRow = document.getElementById('detail-resume');
    this.resumeDetail = document.getElementById('detail-resume-detail');
    this.resumeClearBtn = document.getElementById('detail-resume-clear');

    /** @type {import('../data/catalog.js').CatalogEntry|null} */
    this.entry = null;
    /** @type {import('../data/save-states.js').SaveState[]} */
    this.states = [];

    // Spacer that gives the states list its scroll height. Rows are absolutely
    // positioned against it.
    this.statesContent = document.createElement('div');
    this.statesContent.style.position = 'relative';
    this.statesEl.appendChild(this.statesContent);

    this.scroller = new VirtualScroller({
      viewport: this.statesEl,
      content: this.statesContent,
      axis: 'y',
      itemSize: ROW_HEIGHT,
      overscan: 2,
      scheduler: this.scheduler,
      createNode: () => this._createRow(),
      bindNode: (row, index) => this._bindRow(row, index),
    });

    this.flush = this.flush.bind(this);
    this._wire();
  }

  _wire() {
    for (const el of this.root.querySelectorAll('[data-close]')) {
      el.addEventListener('click', () => this.close());
    }
    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape' && !this.root.hidden) this.close();
    });

    this.playBtn.addEventListener('click', () => {
      if (this.entry) this.onLaunch(this.entry.id);
    });

    this.removeBtn?.addEventListener('click', async () => {
      if (!this.entry || this.entry.source !== 'imported') return;
      const id = this.entry.id;
      this.close();
      await this.onRemoveRom?.(id);
    });

    this.favBtn.addEventListener('click', () => {
      if (!this.entry) return;
      const now = toggleFavorite(this.entry.id);
      this.entry.favorite = now;
      this._syncFavButton();
      this.onDataChanged();
    });

    this.statesEl.addEventListener('click', (event) => {
      const button = event.target.closest('.state-row__load');
      if (!button || !this.entry) return;
      const slot = Number(button.dataset.slot);
      if (Number.isFinite(slot)) this.onLoadState(this.entry.id, slot);
    });

    this.resumeClearBtn?.addEventListener('click', async () => {
      if (!this.entry) return;
      await this.onClearResume?.(this.entry.id);
      this._syncResume();
    });

    this.coreSelect?.addEventListener('change', () => {
      if (!this.entry) return;
      // Stored against the system, so it applies to the whole library for that
      // system rather than to this one game.
      setCorePreference(this.entry.systemId, this.coreSelect.value || null);
      this._syncCorePicker();
    });
  }

  /**
   * Fills the core picker, or hides it when the system has only one option.
   *
   * The list comes from the registry, so a system whose second core is removed from
   * the manifest quietly loses its picker instead of offering something that no
   * longer exists.
   */
  _syncCorePicker() {
    if (!this.coreRow || !this.coreSelect) return;
    const entry = this.entry;
    const candidates = entry ? this.coresForSystem(entry.systemId) : [];

    if (!entry || candidates.length < 2) {
      this.coreRow.hidden = true;
      return;
    }
    this.coreRow.hidden = false;

    const stored = getCorePreference(entry.systemId);
    const defaultCore = candidates[0];

    // Rebuilt on open rather than recycled: the option count is the number of cores
    // for one system, and the sheet shows one game at a time.
    this.coreSelect.textContent = '';
    const auto = document.createElement('option');
    auto.value = '';
    auto.textContent = `Default (${defaultCore.name})`;
    this.coreSelect.appendChild(auto);
    for (const core of candidates) {
      const option = document.createElement('option');
      option.value = core.id;
      option.textContent =
        core.kind === 'libretro' ? core.name : `${core.name} (diagnostic stand-in)`;
      this.coreSelect.appendChild(option);
    }
    // A stored id that is no longer offered falls back to the default entry, which
    // matches what the registry will actually do at launch.
    this.coreSelect.value = candidates.some((c) => c.id === stored) ? stored : '';

    const chosen = candidates.find((c) => c.id === this.coreSelect.value) ?? defaultCore;
    this.coreNote.textContent =
      chosen.kind === 'libretro'
        ? `${candidates.length} cores can run this system. Using ${chosen.name}.`
        : `${chosen.name} is still a placeholder — it renders the diagnostic pattern, not the game.`;
  }

  // ----------------------------------------------------------------- state rows

  _createRow() {
    const row = document.createElement('div');
    row.className = 'state-row';
    row.setAttribute('role', 'listitem');

    const slot = document.createElement('span');
    slot.className = 'state-row__slot';

    const meta = document.createElement('div');
    meta.className = 'state-row__meta';
    const when = document.createElement('p');
    when.className = 'state-row__when';
    const detail = document.createElement('p');
    detail.className = 'state-row__detail';
    meta.append(when, detail);

    const load = document.createElement('button');
    load.type = 'button';
    load.className = 'textbtn state-row__load';
    load.textContent = 'Load';

    row.append(slot, meta, load);
    row._refs = { slot, when, detail, load };
    return row;
  }

  _bindRow(row, index) {
    const state = this.states[index];
    const r = row._refs;
    if (!state) {
      row.hidden = true;
      return;
    }
    row.hidden = false;
    r.slot.textContent = String(state.slot);
    r.when.textContent = `${state.auto ? 'Auto save' : 'Manual save'} · ${formatAge(state.createdAt)}`;
    // Naming the core that wrote it is what lets someone understand a refusal to
    // load: a state is only meaningful to the build that produced it, and seeing
    // "Snes9x 1.63" next to a state explains why 1.64 will not take it.
    const core = state.coreVersion
      ? ` · ${state.coreName} ${state.coreVersion}`
      : state.synthetic
        ? ' · catalogue placeholder'
        : '';
    r.detail.textContent =
      `frame ${state.frame.toLocaleString()} · ${state.sizeKb.toFixed(1)} KB${core}`;
    r.load.dataset.slot = String(state.slot);
    // An auto-save is restored by launching, so offering "Load" for it inside the
    // same sheet is a button that duplicates the Play button above it.
    r.load.hidden = state.auto && !state.synthetic;
  }

  // --------------------------------------------------------------- open / close

  open(entryId) {
    const entry = entryById(entryId);
    if (!entry) return;
    this.entry = entry;
    this.states = listFor(entryId);

    const system = getSystem(entry.systemId);
    const locked = system?.phase === 2;

    this.artEl.style.background = artFor(entry);
    document.getElementById('detail-badge').textContent = system?.short ?? entry.systemId;
    document.getElementById('detail-glyph').textContent = glyphFor(entry);
    this.titleEl.textContent = entry.title;
    this.metaEl.textContent = metaLineFor(entry);
    this.blurbEl.textContent = locked
      ? `${entry.blurb} — ${system.name} needs the JIT-capable native build; it is listed here but cannot run in the browser.`
      : entry.blurb;

    this.playBtn.disabled = locked;
    this.playBtn.textContent = locked ? 'Phase 2 only' : 'Play';
    // Only imported ROMs can be removed; built-ins ship with the app and synthetic
    // entries have nothing to delete.
    if (this.removeBtn) this.removeBtn.hidden = entry.source !== 'imported';

    const provenance = document.getElementById('detail-provenance');
    if (provenance) {
      if (entry.real) {
        provenance.hidden = false;
        provenance.textContent =
          entry.source === 'builtin'
            ? `Ships with the app · ${entry.filename} · runs on the real core`
            : `Imported · ${entry.filename} · ${(entry.sizeMb * 1024).toFixed(0)} KB in local storage`;
      } else {
        provenance.hidden = false;
        provenance.textContent =
          'Catalogue placeholder — no ROM data. Add your own file to play this system.';
      }
    }
    this._syncFavButton();
    this._syncCorePicker();
    this._syncResume();

    this.statesCountEl.textContent =
      this.states.length === 0 ? 'none yet' : `${this.states.length.toLocaleString()} saved`;

    this.root.hidden = false;
    // Measure only once visible: a hidden element reports zero height, which would
    // make the scroller think nothing is on screen.
    this.scroller.measure();
    this.statesEl.scrollTop = 0;
    this.scroller.setCount(this.states.length);
    this.scroller.refresh();
    this.scheduler.wake(3);

    this.playBtn.focus({ preventScroll: true });
  }

  close() {
    this.root.hidden = true;
    this.entry = null;
  }

  get isOpen() {
    return !this.root.hidden;
  }

  _syncFavButton() {
    const fav = this.entry?.favorite;
    this.favBtn.textContent = fav ? 'Remove from favorites' : 'Add to favorites';
  }

  /**
   * Shows the resume point, if there is one.
   *
   * This row is the only escape from automatic resuming. Launching a game always
   * restores its checkpoint, so without a way to discard it there would be no way to
   * start a game over — which is a reasonable thing to want and an unreasonable thing
   * to make someone clear their browser storage for.
   */
  _syncResume() {
    if (!this.resumeRow) return;
    const record = this.entry ? autoStateFor(this.entry.id) : null;
    if (!record) {
      this.resumeRow.hidden = true;
      return;
    }
    this.resumeRow.hidden = false;
    const core = record.coreVersion
      ? `${record.coreName} ${record.coreVersion}`
      : (record.coreName ?? record.coreId ?? 'unknown core');
    this.resumeDetail.textContent =
      `frame ${record.frame.toLocaleString()} · ${record.sizeKb.toFixed(1)} KB · ` +
      `${formatAge(record.createdAt)} · ${core}`;
  }

  /** Refreshes the list after a new state is written while the sheet is open. */
  reloadStates() {
    if (!this.entry) return;
    this._syncResume();
    this.states = listFor(this.entry.id);
    this.statesCountEl.textContent = `${this.states.length.toLocaleString()} saved`;
    this.scroller.setCount(this.states.length);
    this.scroller.refresh();
    this.scheduler.wake(2);
  }

  flush() {
    if (!this.root.hidden) this.scroller.flush();
  }
}
