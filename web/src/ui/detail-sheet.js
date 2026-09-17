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
import { formatAge, listFor } from '../data/save-states.js';

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
  constructor({ onLaunch, onLoadState, onDataChanged, scheduler }) {
    this.onLaunch = onLaunch;
    this.onLoadState = onLoadState;
    this.onDataChanged = onDataChanged;
    this.scheduler = scheduler;

    this.root = document.getElementById('detail-sheet');
    this.artEl = document.getElementById('detail-art');
    this.titleEl = document.getElementById('detail-title');
    this.metaEl = document.getElementById('detail-meta');
    this.blurbEl = document.getElementById('detail-blurb');
    this.playBtn = document.getElementById('detail-play');
    this.favBtn = document.getElementById('detail-favorite');
    this.statesEl = document.getElementById('detail-states');
    this.statesCountEl = document.getElementById('detail-states-count');

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
    r.detail.textContent = `frame ${state.frame.toLocaleString()} · ${state.sizeKb.toFixed(1)} KB`;
    r.load.dataset.slot = String(state.slot);
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
    this._syncFavButton();

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

  /** Refreshes the list after a new state is written while the sheet is open. */
  reloadStates() {
    if (!this.entry) return;
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
