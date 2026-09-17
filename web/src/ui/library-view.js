/**
 * Library view: hero banner, Netflix-style shelves, and an all-games grid.
 *
 * ## Two-dimensional virtualisation
 *
 * A shelf layout is a nested windowing problem, and both axes are windowed here:
 *
 *   - **Vertically**, a `VirtualScroller` over the shelf list. Four or five shelf
 *     nodes exist regardless of how many categories there are; scrolling rebinds
 *     them.
 *   - **Horizontally**, each shelf node owns its own `VirtualScroller` over that
 *     shelf's cards. Roughly a dozen card nodes per shelf, whether the shelf holds
 *     12 games or 1,200.
 *
 * The shelf node pool is fixed, so the horizontal scrollers are created once, with
 * the nodes, and rebound on recycle. Creating a scroller (and its card pool) per
 * shelf *appearance* would allocate on the scroll path, which is precisely what
 * this design exists to avoid.
 *
 * Total card nodes in the document: `shelfPool * cardsPerShelf` ≈ 5 × 13 ≈ 65,
 * against a 4,800-entry catalogue. The status bar prints the real count so the
 * claim is checkable at a glance.
 *
 * ## Grid mode
 *
 * The same primitive with a row-of-columns renderer: one pooled node per *row*,
 * each holding a fixed number of cards. Recycling a row recycles its cards, so the
 * pool is rows rather than individual cards and the column count only changes on
 * resize.
 */

import { VirtualScroller } from './virtual-scroller.js';
import { createCard, bindCard } from './card.js';
import { artFor, metaLineFor } from './art.js';
import {
  allIndices,
  buildShelves,
  catalogSize,
  entryAt,
  favoriteIndices,
  featuredIndex,
  search,
} from '../data/catalog.js';

/** Reads a numeric CSS custom property so JS and CSS cannot disagree on sizes. */
function cssPx(name, fallback) {
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name);
  const value = Number.parseFloat(raw);
  return Number.isFinite(value) ? value : fallback;
}

export class LibraryView {
  /**
   * @param {object} opts
   * @param {(entryId: string) => void} opts.onOpenDetails
   * @param {(entryId: string) => void} opts.onLaunch
   * @param {{ wake: (frames?: number) => void }} opts.scheduler
   */
  constructor({ onOpenDetails, onLaunch, scheduler }) {
    this.onOpenDetails = onOpenDetails;
    this.onLaunch = onLaunch;
    this.scheduler = scheduler;

    this.root = document.getElementById('view-library');
    this.scrollEl = document.getElementById('library-scroll');
    this.topbar = document.getElementById('topbar');
    this.shelvesEl = document.getElementById('shelves-viewport');
    this.gridWrapEl = document.getElementById('grid-viewport');
    this.emptyEl = document.getElementById('library-empty');
    this.searchInput = document.getElementById('search-input');

    /** 'shelves' | 'grid' | 'favorites' */
    this.mode = 'shelves';
    this.query = '';
    /** @type {{id:string,title:string,indices:Uint32Array}[]} */
    this.shelves = [];
    /** Index list backing grid mode. */
    this.gridIndices = allIndices();
    this.gridColumns = 0;

    /** @type {VirtualScroller|null} */
    this.shelfScroller = null;
    /** @type {VirtualScroller|null} */
    this.gridScroller = null;
    /** Horizontal scrollers owned by pooled shelf nodes. */
    this.rowScrollers = [];

    this.geom = this._readGeometry();
    this.flush = this.flush.bind(this);
  }

  _readGeometry() {
    return {
      cardW: cssPx('--card-w', 158),
      cardGap: cssPx('--card-gap', 12),
      shelfH: cssPx('--shelf-h', 296),
      gridRowH: cssPx('--grid-row-h', 262),
      gutter: cssPx('--sp-6', 32),
    };
  }

  mount() {
    this.shelves = buildShelves();
    this._bindHero();
    this._buildShelfScroller();
    // The grid's node pool is built on first use, not at boot: a hidden viewport
    // measures zero, so its pool would be sized from nothing and its ~60 card
    // nodes would sit in the document doing nothing for users who never leave the
    // home view.
    this._wireEvents();
    this.setMode('shelves');
    this._updateSearchPlaceholder();
  }

  // -------------------------------------------------------------------- events

  _wireEvents() {
    // One delegated handler for every card in the view. Per-card listeners would
    // have to be attached and detached on every recycle.
    this.scrollEl.addEventListener('click', (event) => {
      const card = event.target.closest('.card');
      if (!card || card.hidden) return;
      const id = card.dataset.entryId;
      if (id) this.onOpenDetails(id);
    });

    // Double-click launches directly, for people who know what they want.
    this.scrollEl.addEventListener('dblclick', (event) => {
      const card = event.target.closest('.card');
      if (!card || card.dataset.locked === 'true') return;
      const id = card.dataset.entryId;
      if (id) this.onLaunch(id);
    });

    this.scrollEl.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter' && event.key !== ' ') return;
      const card = event.target.closest?.('.card');
      if (!card) return;
      event.preventDefault();
      const id = card.dataset.entryId;
      if (id) this.onOpenDetails(id);
    });

    // Solid top bar once the hero has scrolled away.
    this.scrollEl.addEventListener(
      'scroll',
      () => {
        const scrolled = this.scrollEl.scrollTop > 24;
        this.topbar.classList.toggle('is-scrolled', scrolled);
        this.scheduler.wake(2);
      },
      { passive: true },
    );

    for (const link of document.querySelectorAll('.navlink')) {
      link.addEventListener('click', () => {
        this.searchInput.value = '';
        this.query = '';
        this.setMode(link.dataset.mode);
      });
    }

    let searchTimer = 0;
    this.searchInput.addEventListener('input', () => {
      // Debounced: a keystroke should not trigger a full re-window mid-word.
      clearTimeout(searchTimer);
      searchTimer = setTimeout(() => this.setQuery(this.searchInput.value), 120);
    });

    document.getElementById('hero-play').addEventListener('click', () => {
      if (this.featured) this.onLaunch(this.featured.id);
    });
    document.getElementById('hero-details').addEventListener('click', () => {
      if (this.featured) this.onOpenDetails(this.featured.id);
    });

    // Column count depends on width, so grid geometry is resize-dependent.
    this._resizeObserver = new ResizeObserver(() => {
      this.geom = this._readGeometry();
      if (this.mode !== 'shelves') this._syncGridColumns();
      this.scheduler.wake(2);
    });
    this._resizeObserver.observe(this.scrollEl);
  }

  // ---------------------------------------------------------------------- hero

  _bindHero() {
    const entry = entryAt(featuredIndex());
    this.featured = entry;
    if (!entry) return;
    document.getElementById('hero-art').style.setProperty('--art', artFor(entry));
    document.getElementById('hero-eyebrow').textContent =
      entry.progress > 0 ? 'Continue playing' : 'Featured';
    document.getElementById('hero-title').textContent = entry.title;
    document.getElementById('hero-meta').textContent = metaLineFor(entry);
    document.getElementById('hero-blurb').textContent = entry.blurb;
  }

  // ------------------------------------------------------------------- shelves

  _buildShelfScroller() {
    this.shelfScroller = new VirtualScroller({
      viewport: this.scrollEl,
      content: this.shelvesEl,
      axis: 'y',
      itemSize: this.geom.shelfH,
      overscan: 1,
      scheduler: this.scheduler,
      createNode: () => this._createShelfNode(),
      bindNode: (node, index) => this._bindShelfNode(node, index),
      onRange: () => this._updateStatus(),
    });
    this.shelfScroller.setCount(this.shelves.length);
  }

  _createShelfNode() {
    const shelf = document.createElement('section');
    shelf.className = 'shelf';

    const head = document.createElement('div');
    head.className = 'shelf__head';
    const title = document.createElement('h2');
    title.className = 'shelf__title';
    const count = document.createElement('span');
    count.className = 'shelf__count';
    head.append(title, count);

    const body = document.createElement('div');
    body.className = 'shelf__body';

    const prev = document.createElement('button');
    prev.type = 'button';
    prev.className = 'shelf__arrow shelf__arrow--prev';
    prev.setAttribute('aria-label', 'Scroll left');
    prev.textContent = '‹';

    const next = document.createElement('button');
    next.type = 'button';
    next.className = 'shelf__arrow shelf__arrow--next';
    next.setAttribute('aria-label', 'Scroll right');
    next.textContent = '›';

    const track = document.createElement('div');
    track.className = 'shelf__track no-scrollbar';
    const spacer = document.createElement('div');
    spacer.className = 'shelf__spacer';
    track.appendChild(spacer);

    body.append(prev, track, next);
    shelf.append(head, body);

    // The shelf's own horizontal window. Created with the node and reused for
    // every shelf this node is later recycled into.
    const rowScroller = new VirtualScroller({
      viewport: track,
      content: spacer,
      axis: 'x',
      itemSize: this.geom.cardW,
      gap: this.geom.cardGap,
      overscan: 2,
      scheduler: this.scheduler,
      createNode: createCard,
      bindNode: (card, position) => {
        // `position` indexes this shelf's list; the card wants a catalogue index.
        const indices = shelf._indices;
        if (!indices || position >= indices.length) {
          card.hidden = true;
          return;
        }
        bindCard(card, indices[position]);
      },
    });

    prev.addEventListener('click', () => rowScroller.scrollByPage(-1));
    next.addEventListener('click', () => rowScroller.scrollByPage(1));

    shelf._scroller = rowScroller;
    shelf._indices = null;
    this.rowScrollers.push(rowScroller);

    // Track scroll must not bubble into the vertical scroller's wake logic twice;
    // the scroller already listens, so nothing else is needed here.
    return shelf;
  }

  _bindShelfNode(node, index) {
    const shelf = this.shelves[index];
    if (!shelf) {
      node.hidden = true;
      return;
    }
    node.hidden = false;
    node.dataset.shelfId = shelf.id;
    node.querySelector('.shelf__title').textContent = shelf.title;
    node.querySelector('.shelf__count').textContent = `${shelf.indices.length} titles`;

    node._indices = shelf.indices;
    const scroller = node._scroller;
    // Reset the horizontal position: this node just became a different shelf, and
    // inheriting the previous shelf's scroll offset would look like a glitch.
    scroller.viewport.scrollLeft = 0;
    scroller.setCount(shelf.indices.length);
    scroller.refresh();
  }

  // ---------------------------------------------------------------------- grid

  /** Builds the grid on first use. Idempotent. */
  _ensureGrid() {
    if (this.gridEl) return;
    this.gridHeadEl = document.createElement('div');
    this.gridHeadEl.className = 'grid__head';
    const title = document.createElement('h2');
    title.className = 'grid__title';
    title.textContent = 'All games';
    this.gridCountEl = document.createElement('span');
    this.gridCountEl.className = 'grid__count';
    this.gridHeadEl.append(title, this.gridCountEl);

    this.gridEl = document.createElement('div');
    this.gridEl.className = 'grid';

    this.gridWrapEl.append(this.gridHeadEl, this.gridEl);
    this._syncGridColumns();
  }

  /**
   * Recreates the grid scroller when the column count changes.
   *
   * Rebuilding is the honest option: a row node's card children are structural, so
   * changing columns changes the node type. It only happens on resize, never while
   * scrolling.
   */
  _syncGridColumns() {
    if (!this.gridEl) return;
    const available = this.gridEl.clientWidth || this.scrollEl.clientWidth - this.geom.gutter * 2;
    const stride = this.geom.cardW + this.geom.cardGap;
    const columns = Math.max(1, Math.floor((available + this.geom.cardGap) / stride));
    if (columns === this.gridColumns && this.gridScroller) return;

    this.gridColumns = columns;
    this.gridScroller?.destroy();

    this.gridScroller = new VirtualScroller({
      viewport: this.scrollEl,
      content: this.gridEl,
      axis: 'y',
      itemSize: this.geom.gridRowH,
      overscan: 1,
      scheduler: this.scheduler,
      createNode: () => {
        const row = document.createElement('div');
        row.className = 'grid__row';
        const cards = [];
        for (let c = 0; c < this.gridColumns; c++) {
          const card = createCard();
          cards.push(card);
          row.appendChild(card);
        }
        row._cards = cards;
        return row;
      },
      bindNode: (row, rowIndex) => {
        const start = rowIndex * this.gridColumns;
        for (let c = 0; c < row._cards.length; c++) {
          const listIndex = start + c;
          const card = row._cards[c];
          if (listIndex >= this.gridIndices.length) {
            card.hidden = true;
            continue;
          }
          bindCard(card, this.gridIndices[listIndex]);
        }
      },
      onRange: () => this._updateStatus(),
    });

    this._applyGridIndices(this.gridIndices);
  }

  _applyGridIndices(indices) {
    this.gridIndices = indices;
    const rows = Math.ceil(indices.length / this.gridColumns);
    this.gridScroller.setCount(rows);
    this.gridScroller.refresh();
    this.gridCountEl.textContent = `${indices.length.toLocaleString()} titles`;
    this.emptyEl.hidden = indices.length > 0;
    this._updateStatus();
  }

  // -------------------------------------------------------------- mode / query

  setMode(mode) {
    this.mode = mode;
    for (const link of document.querySelectorAll('.navlink')) {
      link.classList.toggle('is-active', link.dataset.mode === mode);
    }

    const showShelves = mode === 'shelves';
    document.getElementById('hero').hidden = !showShelves;
    this.shelvesEl.hidden = !showShelves;
    this.gridWrapEl.hidden = showShelves;
    this.emptyEl.hidden = true;
    this.scrollEl.scrollTop = 0;

    if (showShelves) {
      this.shelfScroller.measure();
      this.shelfScroller.refresh();
    } else {
      this._ensureGrid();
      const indices =
        mode === 'favorites'
          ? favoriteIndices()
          : this.query
            ? search(this.query)
            : allIndices();
      this.gridHeadEl.querySelector('.grid__title').textContent =
        mode === 'favorites' ? 'Favorites' : this.query ? `Results for “${this.query}”` : 'All games';
      this._syncGridColumns();
      this.gridScroller.measure();
      this._applyGridIndices(indices);
    }
    this.scheduler.wake(3);
    this._updateStatus();
  }

  setQuery(query) {
    this.query = query.trim();
    if (this.query) {
      // Searching implies browsing everything, so leave shelf mode the way a
      // streaming app does.
      this.setMode('grid');
    } else if (this.mode === 'grid') {
      this.setMode('shelves');
    }
  }

  /** Recomputes shelves after data changes (a launch updates "Continue playing"). */
  refreshData() {
    this.shelves = buildShelves();
    this.shelfScroller.setCount(this.shelves.length);
    this.shelfScroller.refresh();
    this._bindHero();
    if (this.mode !== 'shelves' && this.gridScroller) {
      this._applyGridIndices(
        this.mode === 'favorites'
          ? favoriteIndices()
          : this.query
            ? search(this.query)
            : allIndices(),
      );
    }
    this.scheduler.wake(2);
  }

  // --------------------------------------------------------------------- frame

  /** Called once per frame by the shared loop. Cheap when nothing is dirty. */
  flush() {
    if (this.mode === 'shelves') {
      this.shelfScroller?.flush();
      // Only shelves currently bound to a node need flushing; the rest have no
      // nodes to reconcile.
      for (let i = 0; i < this.rowScrollers.length; i++) this.rowScrollers[i].flush();
    } else {
      this.gridScroller?.flush();
    }
  }

  // -------------------------------------------------------------------- status

  _updateSearchPlaceholder() {
    this.searchInput.placeholder = `Search ${catalogSize.toLocaleString()} titles…`;
  }

  _updateStatus() {
    const catalogEl = document.getElementById('status-catalog');
    const domEl = document.getElementById('status-dom');
    if (!catalogEl || !domEl) return;

    const cards = document.querySelectorAll('.card').length;
    const shelfNodes = this.shelfScroller?.nodeCount ?? 0;
    catalogEl.textContent = `${catalogSize.toLocaleString()} titles indexed`;
    domEl.textContent =
      this.mode === 'shelves'
        ? `DOM: ${cards} cards in ${shelfNodes} shelf nodes`
        : `DOM: ${cards} cards in ${this.gridScroller?.nodeCount ?? 0} row nodes`;
  }
}
