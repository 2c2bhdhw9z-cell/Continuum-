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
import { attachCoreMenuGestures } from './core-menu.js';
import { artFor, metaLineFor } from './art.js';
import {
  allIndices,
  buildShelves,
  librarySize,
  entryAt,
  favoriteIndices,
  featuredIndex,
  search,
} from '../data/catalog.js';
import { displayUrlFor } from '../data/artwork.js';

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
  constructor({ onOpenDetails, onLaunch, onCoreMenu, onOpenSettings, onRequestImport, scheduler }) {
    this.onOpenDetails = onOpenDetails;
    this.onLaunch = onLaunch;
    /**
     * Asked to open the subcore picker for a card. Returns whether it opened, so the
     * gesture layer knows whether to swallow the browser's own context menu.
     */
    this.onCoreMenu = onCoreMenu ?? (() => false);
    this.onOpenSettings = onOpenSettings ?? (() => {});
    this.onRequestImport = onRequestImport ?? (() => {});
    this.scheduler = scheduler;

    this.root = document.getElementById('view-library');
    this.scrollEl = document.getElementById('library-scroll');
    this.topbar = document.getElementById('topbar');
    this.shelvesEl = document.getElementById('shelves-viewport');
    this.gridWrapEl = document.getElementById('grid-viewport');
    this.emptyEl = document.getElementById('library-empty');
    this.emptyTitleEl = document.getElementById('library-empty-title');
    this.emptyHintEl = document.getElementById('library-empty-hint');
    this.emptyImportBtn = document.getElementById('library-empty-import');
    this.heroEl = document.getElementById('hero');
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

    // Right-click / long-press: launch with an alternative core. Delegated, because
    // cards are recycled and per-card listeners would churn on every scroll frame.
    attachCoreMenuGestures(this.scrollEl, ({ card, x, y }) => {
      const id = card.dataset.entryId;
      return id ? this.onCoreMenu(id, x, y) : false;
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
        // Settings is a sheet, not a view. Treating it as a fourth mode would blank
        // the library behind it and leave the tab strip claiming you had navigated
        // away from a list that is still right there when the sheet closes.
        if (link.dataset.mode === 'settings') {
          this.onOpenSettings();
          return;
        }
        this.searchInput.value = '';
        this.query = '';
        this.setMode(link.dataset.mode);
      });
    }

    this.emptyImportBtn?.addEventListener('click', () => this.onRequestImport());

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

  /**
   * Binds the hero, or hides it.
   *
   * With no synthetic catalogue there is a state that could not previously exist: an
   * empty library. A hero banner featuring nothing is worse than no banner, so it is
   * removed from the layout entirely and the empty state does the talking.
   */
  _bindHero() {
    const index = featuredIndex();
    const entry = index >= 0 ? entryAt(index) : null;
    this.featured = entry;
    if (this.heroEl) this.heroEl.hidden = !entry || this.mode !== 'shelves';
    if (!entry) return;

    const artEl = document.getElementById('hero-art');
    artEl.style.setProperty('--art', artFor(entry));
    // The hero uses a background image rather than an <img>: it is one element that
    // never recycles, so there is no pool to keep uniform and no decode to sequence.
    const url = displayUrlFor(entry);
    // `JSON.stringify` gives a correctly quoted and backslash-escaped CSS string.
    // `CSS.escape` would be wrong here: it escapes identifiers, not string literals,
    // and would mangle the slashes in a URL.
    artEl.style.backgroundImage = url ? `url(${JSON.stringify(url)})` : '';
    artEl.classList.toggle('has-art', Boolean(url));

    document.getElementById('hero-eyebrow').textContent =
      entry.lastPlayed !== null ? 'Continue playing' : 'Featured';
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
      // The number of shelves is a property of the library, so the pool may be capped
      // to it: a four-cart library builds one shelf node rather than seven.
      capPoolToCount: true,
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
    const subtitle = document.createElement('span');
    subtitle.className = 'shelf__subtitle';
    subtitle.hidden = true;
    head.append(title, count, subtitle);

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
    node.querySelector('.shelf__count').textContent =
      shelf.indices.length === 1 ? '1 title' : `${shelf.indices.length} titles`;
    const subtitle = node.querySelector('.shelf__subtitle');
    subtitle.hidden = !shelf.subtitle;
    subtitle.textContent = shelf.subtitle ?? '';

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
      capPoolToCount: true,
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
    this.gridCountEl.textContent =
      indices.length === 1 ? '1 title' : `${indices.length.toLocaleString()} titles`;
    this._syncEmptyState(indices.length);
    this._updateStatus();
  }

  /**
   * Chooses which "nothing here" message to show, if any.
   *
   * Three genuinely different situations, and one message for all of them would be
   * wrong in two: an empty library needs an import button, an empty Favorites shelf
   * needs to explain what favourites are, and a search with no hits needs to suggest a
   * different query. Only the first is a dead end.
   */
  _syncEmptyState(visibleCount) {
    const total = librarySize();
    const count = visibleCount ?? (this.mode === 'shelves' ? total : this.gridIndices.length);

    if (total === 0) {
      this.emptyEl.hidden = false;
      this.emptyTitleEl.textContent = 'Your library is empty.';
      this.emptyHintEl.textContent =
        'Add a ROM from this device to get started. Nothing is uploaded — files are stored ' +
        'locally in your browser and stay on your device.';
      if (this.emptyImportBtn) this.emptyImportBtn.hidden = false;
      return;
    }

    if (count > 0) {
      this.emptyEl.hidden = true;
      return;
    }

    this.emptyEl.hidden = false;
    if (this.emptyImportBtn) this.emptyImportBtn.hidden = true;
    if (this.mode === 'favorites') {
      this.emptyTitleEl.textContent = 'No favorites yet.';
      this.emptyHintEl.textContent =
        'Open any game and choose “Add to favorites” to collect it here.';
    } else if (this.query) {
      this.emptyTitleEl.textContent = 'No titles match that search.';
      this.emptyHintEl.textContent = 'Try a system name, a region tag, or part of a title.';
    } else {
      this.emptyTitleEl.textContent = 'Nothing to show.';
      this.emptyHintEl.textContent = 'Add a ROM from this device to fill your library.';
      if (this.emptyImportBtn) this.emptyImportBtn.hidden = false;
    }
  }

  // -------------------------------------------------------------- mode / query

  setMode(mode) {
    this.mode = mode;
    for (const link of document.querySelectorAll('.navlink')) {
      if (link.dataset.mode === 'settings') continue;
      link.classList.toggle('is-active', link.dataset.mode === mode);
    }

    const showShelves = mode === 'shelves';
    this.heroEl.hidden = !showShelves || !this.featured;
    this.shelvesEl.hidden = !showShelves;
    this.gridWrapEl.hidden = showShelves;
    this.emptyEl.hidden = true;
    this.scrollEl.scrollTop = 0;

    if (showShelves) {
      this.shelfScroller.measure();
      this.shelfScroller.refresh();
      this._syncEmptyState();
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

  /**
   * Recomputes everything derived from the library after it changes.
   *
   * Called on import, on delete, after a launch (which reorders "Recently played") and
   * when artwork resolves. Shelves are rebuilt rather than patched because the *set* of
   * shelves is itself data now: importing the first Mega Drive ROM creates a shelf that
   * did not exist a moment ago, and deleting the last one removes it.
   */
  refreshData() {
    this.shelves = buildShelves();
    this.shelfScroller.setCount(this.shelves.length);
    this.shelfScroller.refresh();
    this._bindHero();
    if (this.mode === 'shelves') {
      this._syncEmptyState();
    } else if (this.gridScroller) {
      this._applyGridIndices(
        this.mode === 'favorites'
          ? favoriteIndices()
          : this.query
            ? search(this.query)
            : allIndices(),
      );
    }
    this._updateSearchPlaceholder();
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
    const total = librarySize();
    // Recomputed on every library change: this used to be a constant read once at
    // module load, which was fine for a fixed 4,800-entry catalogue and would now
    // permanently advertise whatever the count happened to be at boot.
    this.searchInput.placeholder =
      total === 0
        ? 'Search your library…'
        : `Search ${total.toLocaleString()} ${total === 1 ? 'title' : 'titles'}…`;
  }

  _updateStatus() {
    const catalogEl = document.getElementById('status-catalog');
    const domEl = document.getElementById('status-dom');
    if (!catalogEl || !domEl) return;

    const cards = document.querySelectorAll('.card').length;
    const shelfNodes = this.shelfScroller?.nodeCount ?? 0;
    const total = librarySize();
    catalogEl.textContent = `${total.toLocaleString()} ${total === 1 ? 'title' : 'titles'} in library`;
    domEl.textContent =
      this.mode === 'shelves'
        ? `DOM: ${cards} cards in ${shelfNodes} shelf nodes`
        : `DOM: ${cards} cards in ${this.gridScroller?.nodeCount ?? 0} row nodes`;
  }
}
