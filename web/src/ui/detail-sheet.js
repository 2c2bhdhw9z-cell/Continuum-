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
import { entryById, toggleFavorite, formatBytes } from '../data/catalog.js';
import {
  displayUrlFor,
  storeManualArtwork,
  clearArtwork,
  scrapeArtwork,
  describeTier,
} from '../data/artwork.js';
import { getSystem } from '../data/systems.js';
import { getCorePreference, setCorePreference } from '../data/core-prefs.js';
import { formatAge, listFor, autoStateFor } from '../data/save-states.js';
import { toast } from './toast.js';

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
    this.artImgEl = document.getElementById('detail-art-img');
    this.artNoteEl = document.getElementById('detail-art-note');
    this.artChangeBtn = document.getElementById('detail-art-change');
    this.artResetBtn = document.getElementById('detail-art-reset');
    this.artInput = document.getElementById('detail-art-input');
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
      // A game with two save states gets two rows, not ten. The count changes when a
      // different game is opened, never while this list is being scrolled.
      capPoolToCount: true,
      createNode: () => this._createRow(),
      bindNode: (row, index) => this._bindRow(row, index),
      // Wipes a row's text as it leaves the window. The scroller already moves it far
      // off-screen, so this is not what stops it being seen — it is so that the pool
      // holds no copy of a game's save history once that game's sheet has closed.
      // Anyone inspecting the DOM, and anything reading it, sees empty rows.
      unbindNode: (row) => {
        row.hidden = true;
        row._refs.slot.textContent = '';
        row._refs.when.textContent = '';
        row._refs.detail.textContent = '';
        row._refs.load.hidden = true;
        delete row._refs.load.dataset.slot;
      },
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

    // ---- Tier 5: artwork the user picks themselves ----
    this.artChangeBtn?.addEventListener('click', () => this.artInput?.click());

    this.artInput?.addEventListener('change', async () => {
      const file = this.artInput.files?.[0];
      // Cleared first, so choosing the same file twice fires `change` again.
      this.artInput.value = '';
      if (!file || !this.entry) return;
      const entryId = this.entry.id;
      try {
        await storeManualArtwork(entryId, file);
        this._syncArtwork();
        this.onDataChanged();
        toast('Artwork updated', file.name);
      } catch (err) {
        toast('Could not use that image', String(err?.message ?? err), { kind: 'warn' });
      }
    });

    // Reverting has to be possible, and it has to mean "try again" rather than "be
    // blank forever": clearing drops the stored image *and* the negative cache, so a
    // scrape can find something that was not there when the ROM was first imported.
    this.artResetBtn?.addEventListener('click', async () => {
      if (!this.entry) return;
      const entry = this.entry;
      try {
        await clearArtwork(entry.id);
        this._syncArtwork();
        this.onDataChanged();
        const found = await scrapeArtwork(entry);
        if (this.entry?.id === entry.id) this._syncArtwork();
        this.onDataChanged();
        toast(
          found ? 'Artwork restored' : 'Artwork cleared',
          found
            ? 'Found cover art in the libretro archive.'
            : 'Showing the generated console plate. A thumbnail will be captured next time you play.',
        );
      } catch (err) {
        toast('Could not clear artwork', String(err?.message ?? err), { kind: 'warn' });
      }
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
    // A row may only ever show a state belonging to the game currently on screen.
    // `listFor` is keyed by game id so this should be unreachable, but it is the last
    // point before someone else's save history reaches a pixel, and that is exactly
    // where an invariant is worth restating rather than assuming.
    if (!state || !this.entry || state.gameId !== this.entry.id) {
      row.hidden = true;
      if (state && this.entry) {
        console.warn(
          `[detail] refused to render state for ${state.gameId} under ${this.entry.id}`,
        );
      }
      return;
    }
    row.hidden = false;
    r.slot.textContent = String(state.slot);
    r.when.textContent = `${state.auto ? 'Auto save' : 'Manual save'} · ${formatAge(state.createdAt)}`;
    // Naming the core that wrote it is what lets someone understand a refusal to
    // load: a state is only meaningful to the build that produced it, and seeing
    // "Snes9x 1.63" next to a state explains why 1.64 will not take it.
    const core = state.coreVersion ? ` · ${state.coreName} ${state.coreVersion}` : '';
    r.detail.textContent =
      `frame ${state.frame.toLocaleString()} · ${state.sizeKb.toFixed(1)} KB${core}`;
    r.load.dataset.slot = String(state.slot);
    // An auto-save is restored by launching, so offering "Load" for it inside the
    // same sheet is a button that duplicates the Play button above it.
    r.load.hidden = state.auto;
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
    // Only imported ROMs can be removed; built-in carts ship with the app.
    if (this.removeBtn) this.removeBtn.hidden = entry.source !== 'imported';

    const provenance = document.getElementById('detail-provenance');
    if (provenance) {
      provenance.hidden = false;
      provenance.textContent =
        entry.source === 'builtin'
          ? `Ships with the app · ${entry.filename} · runs on the real core`
          : `Imported · ${entry.filename} · ${formatBytes(entry.sizeBytes)} held in this browser`;
    }
    this._syncArtwork();
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
    // Flushed synchronously, before this function returns and the browser paints.
    // Leaving it to the frame loop would show one frame of whatever the pool happened
    // to be holding, which for a list keyed by game is one frame of the wrong game.
    this.scroller.flush();
    this.scheduler.wake(3);

    this.playBtn.focus({ preventScroll: true });
  }

  /**
   * Closes the sheet and blanks it completely.
   *
   * Every route out of the sheet lands here — the close button, the backdrop scrim and
   * the Escape key all share the one handler — so this is the only place the reset needs
   * to live.
   *
   * The reset is not housekeeping. `flush()` deliberately does nothing while the sheet is
   * hidden, so anything left behind sits in the DOM until the next flush and is visible
   * for a frame when the sheet reopens for a *different* game. Blanking on the way out
   * means the worst case a user can ever see is an empty sheet, rather than a flash of
   * someone else's title, cover and save history.
   */
  close() {
    this.root.hidden = true;
    this._blank();
  }

  /**
   * Zeroes every field the sheet renders.
   *
   * Ordered the way the panel reads, so it is obvious at a glance whether something has
   * been missed — and if a field is added to `open()` without being added here, the
   * browser suite's state-bleed checks are what catch it.
   */
  _blank() {
    this.entry = null;
    this.states = [];

    // Artwork: both layers, plus its provenance line and the reset affordance.
    this.artEl.style.background = '';
    this.artImgEl?.removeAttribute('src');
    if (this.artImgEl) this.artImgEl.hidden = true;
    if (this.artNoteEl) this.artNoteEl.textContent = '';
    if (this.artResetBtn) this.artResetBtn.hidden = true;
    const badge = document.getElementById('detail-badge');
    const glyph = document.getElementById('detail-glyph');
    if (badge) badge.textContent = '';
    if (glyph) glyph.textContent = '';

    // Identity and metadata.
    this.titleEl.textContent = '';
    this.metaEl.textContent = '';
    this.blurbEl.textContent = '';
    const provenance = document.getElementById('detail-provenance');
    if (provenance) {
      provenance.hidden = true;
      provenance.textContent = '';
    }

    // Actions: back to their default state, not the last game's.
    this.playBtn.disabled = false;
    this.playBtn.textContent = 'Play';
    this.favBtn.textContent = 'Add to favorites';
    if (this.removeBtn) this.removeBtn.hidden = true;

    // Core picker. Emptied as well as hidden: a stale <option> list would otherwise be
    // the next game's picker for a frame.
    if (this.coreRow) this.coreRow.hidden = true;
    if (this.coreSelect) this.coreSelect.textContent = '';
    if (this.coreNote) this.coreNote.textContent = '';

    // Resume block.
    if (this.resumeRow) this.resumeRow.hidden = true;
    if (this.resumeDetail) this.resumeDetail.textContent = '';

    // Save-state list. Emptied through the scroller and flushed synchronously, so the
    // rows are parked now rather than on some later frame that only arrives if the sheet
    // is reopened.
    if (this.statesCountEl) this.statesCountEl.textContent = '';
    this.statesEl.scrollTop = 0;
    this.scroller.setCount(0);
    this.scroller.flush();
  }

  get isOpen() {
    return !this.root.hidden;
  }

  _syncFavButton() {
    const fav = this.entry?.favorite;
    this.favBtn.textContent = fav ? 'Remove from favorites' : 'Add to favorites';
  }

  /** Shows whatever tier resolved this entry's cover, and where it came from. */
  _syncArtwork() {
    const entry = this.entry;
    if (!this.artImgEl) return;

    const url = entry ? displayUrlFor(entry) : null;
    if (url) {
      this.artImgEl.src = url;
      this.artImgEl.hidden = false;
    } else {
      this.artImgEl.removeAttribute('src');
      this.artImgEl.hidden = true;
    }

    if (this.artNoteEl) this.artNoteEl.textContent = describeTier(entry?.art);
    // Nothing to reset when the plate is already what is showing.
    if (this.artResetBtn) this.artResetBtn.hidden = !entry?.art;
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
      // Emptied, not just hidden. A hidden element still holds its text, and text that
      // describes another game's checkpoint is one stray `hidden = false` away from
      // being wrong on screen — and it reads as a leak to anyone inspecting the DOM.
      this.resumeDetail.textContent = '';
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
    this.statesCountEl.textContent =
      this.states.length === 0 ? 'none yet' : `${this.states.length.toLocaleString()} saved`;
    this.scroller.setCount(this.states.length);
    this.scroller.refresh();
    if (this.isOpen) this.scroller.flush();
    this.scheduler.wake(2);
  }

  flush() {
    if (!this.root.hidden) this.scroller.flush();
  }
}
