/**
 * VirtualScroller — one-dimensional windowed list with modulo node recycling.
 *
 * Every list in this app is built from this primitive: the vertical stack of
 * shelves, the horizontal strip of cards inside each shelf, the all-games grid
 * (rows of cards), and the save-state list. One implementation, four call sites.
 *
 * ## The guarantee
 *
 * DOM node count is a function of *viewport size*, never of item count. A shelf
 * holding 12 games and a shelf holding 4,800 games allocate exactly the same
 * number of elements. `assertBounded()` makes that testable rather than aspirational.
 *
 * ## Modulo recycling
 *
 * The naive approach appends and removes nodes as the window moves, which means
 * DOM mutation on every scroll frame — the expensive kind of work, since insertion
 * invalidates layout for siblings.
 *
 * Instead, a fixed pool of `poolSize` nodes is created once and *never* detached.
 * Item `i` always lives in slot `i % poolSize`. Because `poolSize` is strictly
 * greater than the number of simultaneously visible items, two visible items can
 * never collide in the same slot. Scrolling therefore costs:
 *
 *   - one `transform` write per newly positioned node, and
 *   - one `bindNode(node, index)` call per node whose index actually changed.
 *
 * No insertions, no removals, no layout thrash. Nodes that scroll out of the
 * window are not hidden either; they are simply positioned off-screen and reused
 * moments later.
 *
 * ## Frame alignment
 *
 * `scroll` events fire faster than frames and must not do layout work directly.
 * The scroller marks itself dirty and lets the app's single `requestAnimationFrame`
 * loop call `flush()`. One loop drives the emulator *and* the UI, so scrolling
 * cannot preempt a frame the emulator needed.
 */

/** Reasonable ceiling so a bug in size maths cannot allocate thousands of nodes. */
const MAX_POOL = 96;

export class VirtualScroller {
  /**
   * @param {object} opts
   * @param {HTMLElement} opts.viewport     Scrollable clipping element.
   * @param {HTMLElement} opts.content      Sized spacer inside the viewport.
   * @param {'x'|'y'}     [opts.axis]       Scroll axis. Default `'y'`.
   * @param {number}      opts.itemSize     Item extent along the axis, in px.
   * @param {number}      [opts.gap]        Gap between items, in px.
   * @param {number}      [opts.overscan]   Extra items rendered beyond the window.
   * @param {number}      [opts.crossSize]  Fixed extent across the axis, in px.
   * @param {() => HTMLElement} opts.createNode  Builds one pooled node.
   * @param {(node: HTMLElement, index: number) => void} opts.bindNode
   *        Populates a node for `index`. Must not change the node's size.
   * @param {(node: HTMLElement) => void} [opts.unbindNode]
   *        Optional teardown when a node leaves the window entirely.
   * @param {(range: {first:number,last:number}) => void} [opts.onRange]
   * @param {{wake: (frames?: number) => void}} [opts.scheduler]
   *        The frame loop. Omitted only in tests.
   * @param {boolean} [opts.capPoolToCount]
   *        Limit the pool to `count` nodes as well as to the viewport. Only valid when
   *        `count` is a property of the data rather than of scroll position — see
   *        `_wantedPool`.
   */
  constructor(opts) {
    this.viewport = opts.viewport;
    this.content = opts.content;
    this.axis = opts.axis === 'x' ? 'x' : 'y';
    this.itemSize = opts.itemSize;
    this.gap = opts.gap ?? 0;
    this.overscan = opts.overscan ?? 2;
    this.crossSize = opts.crossSize ?? 0;
    this.createNode = opts.createNode;
    this.bindNode = opts.bindNode;
    this.unbindNode = opts.unbindNode ?? null;
    this.onRange = opts.onRange ?? null;
    this.scheduler = opts.scheduler ?? null;
    this.capPoolToCount = opts.capPoolToCount === true;

    this.count = 0;
    this.stride = this.itemSize + this.gap;

    /** @type {HTMLElement[]} Pooled nodes, index = slot. */
    this.slots = [];
    /** Index currently bound to each slot, or -1. */
    this.slotIndex = [];
    /** Last transform written per slot, so identical writes are skipped. */
    this.slotOffset = [];

    this.poolSize = 0;
    this.viewportSize = 0;
    /** Distance from the scroll container's origin to `content`'s origin. */
    this.contentOffset = 0;
    this.firstVisible = 0;
    this.lastVisible = -1;
    this.dirty = true;
    this.destroyed = false;

    // Cheap counters, surfaced in the status bar to make the virtualisation
    // claim visible instead of theoretical.
    this.stats = { flushes: 0, binds: 0, transforms: 0 };

    this._onScroll = () => this.markDirty();
    this.viewport.addEventListener('scroll', this._onScroll, { passive: true });

    this._resizeObserver = new ResizeObserver(() => {
      this.measure();
      this.markDirty();
    });
    this._resizeObserver.observe(this.viewport);

    this.measure();
  }

  /** Re-reads viewport geometry and grows the pool if the window got bigger. */
  measure() {
    if (this.destroyed) return;
    this.viewportSize =
      this.axis === 'y' ? this.viewport.clientHeight : this.viewport.clientWidth;

    this._syncContentOffset();
    this._ensurePool();
  }

  /**
   * How many nodes this scroller should own: enough to fill the viewport, and — when
   * `capPoolToCount` is set — never more than there are items to put in them.
   *
   * The cap matters for small collections. Sized from the viewport alone, a library of
   * four test carts still builds seven shelf nodes holding ninety-eight cards, which was
   * invisible while a synthetic catalogue guaranteed fifteen full shelves and is pure
   * waste for a real library that starts at four titles.
   *
   * **The cap is opt-in, and that is the important part.** It may only be used where
   * `count` is a property of the data rather than of scroll position. The shelf list and
   * the grid qualify: their counts change when the library changes. A shelf's *own*
   * horizontal scroller does not — one pooled shelf node is rebound from a 4-item shelf
   * to a 54-item one as the user scrolls, so capping it there would grow the pool
   * mid-scroll and break the invariant this whole class exists to provide: that
   * scrolling adds no DOM nodes, ever.
   */
  _wantedPool() {
    const visible = Math.ceil(this.viewportSize / this.stride) + 1;
    const forViewport = Math.min(visible + this.overscan * 2 + 1, MAX_POOL);
    // Uncapped scrollers are sized from the viewport alone, and *eagerly* — before any
    // count is known. That matters: a shelf's horizontal scroller is built with its
    // shelf node, long before it is bound to a shelf, and deferring its pool until the
    // first bind would mean scrolling new shelves into view added card nodes as it went.
    // Rule 4 says scrolling adds no nodes, so the nodes must already exist.
    if (!this.capPoolToCount) return forViewport;
    return this.count <= 0 ? 0 : Math.min(forViewport, this.count);
  }

  /** Grows the pool toward what is currently needed. Never shrinks — see `_growPool`. */
  _ensurePool() {
    const wanted = this._wantedPool();
    if (wanted > this.poolSize) this._growPool(wanted);
  }

  /**
   * Measures the distance from the start of the scrollable content to the content
   * element. Non-zero when static chrome shares the scroll container (the hero
   * banner above the shelves) or when the viewport is padded (a shelf's left
   * gutter).
   *
   * Uses bounding rects plus the current scroll offset rather than
   * `offsetTop`/`offsetLeft`, because those are each relative to their own
   * `offsetParent` and subtracting them is only valid when both share one. Where
   * they do not — a scroller whose viewport *is* the content's offsetParent, like
   * the save-state list — the difference is nonsense and the window lands
   * off-screen.
   *
   * Recomputed on every dirty flush: chrome above the content can change height
   * (hero resize, font swap) without the viewport itself resizing. Two rect reads,
   * and only transforms are written afterwards, so this does not thrash layout.
   */
  _syncContentOffset() {
    const viewportRect = this.viewport.getBoundingClientRect();
    const contentRect = this.content.getBoundingClientRect();
    this.contentOffset =
      this.axis === 'y'
        ? contentRect.top - viewportRect.top + this.viewport.scrollTop
        : contentRect.left - viewportRect.left + this.viewport.scrollLeft;
  }

  _growPool(size) {
    for (let slot = this.poolSize; slot < size; slot++) {
      const node = this.createNode();
      node.style.position = 'absolute';
      node.dataset.slot = String(slot);
      // Parked outside the viewport until first bound, so an unused pool node
      // is never visible.
      node.style.transform = 'translate3d(0, -99999px, 0)';
      this.slots[slot] = node;
      this.slotIndex[slot] = -1;
      this.slotOffset[slot] = NaN;
      this.content.appendChild(node);
    }
    this.poolSize = size;
  }

  /** Updates item count and the scrollable extent. */
  setCount(count) {
    this.count = Math.max(0, count | 0);
    const total = this.count === 0 ? 0 : this.count * this.stride - this.gap;
    if (this.axis === 'y') {
      this.content.style.height = `${total}px`;
      if (this.crossSize) this.content.style.width = `${this.crossSize}px`;
    } else {
      this.content.style.width = `${total}px`;
    }
    // Every slot's binding is now suspect (item 5 may be a different game).
    this.slotIndex.fill(-1);
    // The pool is capped by the item count, so a list that just grew may now need
    // more nodes than it has. Growing here rather than waiting for a resize is what
    // makes importing the fifth game into a four-game shelf actually show it.
    this._ensurePool();
    this.markDirty();
  }

  /** Marks the window stale; the frame loop calls `flush()` shortly after. */
  markDirty() {
    this.dirty = true;
    // Two frames of wake: one to flush this change, one to catch the scroll
    // event that inevitably follows a momentum scroll.
    if (this.scheduler) this.scheduler.wake(2);
  }

  /** Forces a rebind of every visible node (data changed, not geometry). */
  refresh() {
    this.slotIndex.fill(-1);
    this.markDirty();
  }

  /**
   * Reconciles the DOM with the current scroll position. Idempotent and cheap
   * when nothing moved, so calling it every frame is fine.
   */
  flush() {
    if (this.destroyed || !this.dirty) return;
    this.dirty = false;
    this.stats.flushes++;

    this._syncContentOffset();

    const scroll =
      this.axis === 'y' ? this.viewport.scrollTop : this.viewport.scrollLeft;
    // Scroll position translated into content-local space.
    const local = scroll - this.contentOffset;

    let first = Math.floor(local / this.stride) - this.overscan;
    let last = Math.ceil((local + this.viewportSize) / this.stride) + this.overscan;
    first = Math.max(0, first);
    last = Math.min(this.count - 1, last);

    // A window wider than the pool would alias two visible items into one slot.
    // Clamping is the safe failure: the tail renders blank rather than flickering
    // between two items.
    if (last - first + 1 > this.poolSize) last = first + this.poolSize - 1;

    this.firstVisible = first;
    this.lastVisible = last;

    for (let index = first; index <= last; index++) {
      const slot = index % this.poolSize;
      const node = this.slots[slot];
      const offset = index * this.stride;

      if (this.slotIndex[slot] !== index) {
        this.slotIndex[slot] = index;
        node.dataset.index = String(index);
        this.bindNode(node, index);
        this.stats.binds++;
      }

      if (this.slotOffset[slot] !== offset) {
        this.slotOffset[slot] = offset;
        node.style.transform =
          this.axis === 'y'
            ? `translate3d(0, ${offset}px, 0)`
            : `translate3d(${offset}px, 0, 0)`;
        this.stats.transforms++;
      }
    }

    // Park slots that fell outside the window. Keeping them in the DOM is the
    // whole point; they are just moved out of sight until reused.
    for (let slot = 0; slot < this.poolSize; slot++) {
      const index = this.slotIndex[slot];
      if (index === -1) continue;
      if (index < first || index > last) {
        this.slotIndex[slot] = -1;
        this.slotOffset[slot] = NaN;
        // A recycled node must not keep focus: the element the user was on is
        // about to represent a different item.
        if (this.slots[slot].contains(document.activeElement)) {
          this.viewport.focus({ preventScroll: true });
        }
        if (this.unbindNode) this.unbindNode(this.slots[slot]);
        this.slots[slot].style.transform = 'translate3d(0, -99999px, 0)';
      }
    }

    if (this.onRange) this.onRange({ first, last });
  }

  /** Scrolls so `index` is visible. `align`: `'start' | 'center'`. */
  scrollToIndex(index, { align = 'start', behavior = 'auto' } = {}) {
    const clamped = Math.max(0, Math.min(this.count - 1, index));
    let target = this.contentOffset + clamped * this.stride;
    if (align === 'center') {
      target -= Math.max(0, (this.viewportSize - this.itemSize) / 2);
    }
    const opts =
      this.axis === 'y'
        ? { top: Math.max(0, target), behavior }
        : { left: Math.max(0, target), behavior };
    this.viewport.scrollTo(opts);
    this.markDirty();
  }

  /** Scrolls by whole pages; used by the shelf arrow buttons. */
  scrollByPage(direction, behavior = 'smooth') {
    const page = Math.max(this.stride, this.viewportSize - this.stride);
    const current =
      this.axis === 'y' ? this.viewport.scrollTop : this.viewport.scrollLeft;
    const target = current + direction * page;
    this.viewport.scrollTo(
      this.axis === 'y'
        ? { top: Math.max(0, target), behavior }
        : { left: Math.max(0, target), behavior },
    );
    this.markDirty();
  }

  get nodeCount() {
    return this.poolSize;
  }

  get visibleCount() {
    return Math.max(0, this.lastVisible - this.firstVisible + 1);
  }

  /**
   * Throws if the pool ever exceeds what the viewport could need. Called from the
   * dev status bar; the point is that "no O(n) DOM growth" is checked at runtime
   * rather than trusted.
   */
  assertBounded() {
    const maxNeeded = Math.ceil(this.viewportSize / this.stride) + this.overscan * 2 + 2;
    if (this.poolSize > Math.min(maxNeeded, MAX_POOL)) {
      throw new Error(
        `VirtualScroller pool (${this.poolSize}) exceeds viewport need (${maxNeeded})`,
      );
    }
    return true;
  }

  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    this.viewport.removeEventListener('scroll', this._onScroll);
    this._resizeObserver.disconnect();
    for (const node of this.slots) node.remove();
    this.slots.length = 0;
    this.slotIndex.length = 0;
    this.slotOffset.length = 0;
    this.poolSize = 0;
  }
}
