/**
 * "Play with…" — the subcore picker that opens on a ROM card.
 *
 * A system maps to more than one core, so the library needs a way to say "run this
 * Game Boy game on the other core" without a settings screen. Right-click on a
 * pointer, long-press on touch.
 *
 * One menu element exists for the whole page and is reused, in keeping with the rule
 * that the DOM must not grow with the library. It is rebuilt on open, which is safe
 * here and only here: the option count is the number of cores that declare one
 * system — two, today — not the number of games.
 *
 * The menu never decides anything. It renders the list the registry returned and
 * reports the chosen id back; validation, ordering and the fallback for a stale
 * choice all live in Rust.
 */

const LONG_PRESS_MS = 500;
/** Movement beyond this many pixels means the user is scrolling, not pressing. */
const LONG_PRESS_SLOP = 10;

export class CoreMenu {
  /**
   * @param {object} opts
   * @param {(systemId: string) => Array<any>} opts.coresForSystem manifest entries,
   *   best first, as returned by `CoreLoader.coresForSystem`
   * @param {(systemId: string) => string|null} opts.currentCoreId the id that would
   *   be used right now, preference included
   * @param {(entryId: string, coreId: string) => void} opts.onChoose
   */
  constructor({ coresForSystem, currentCoreId, onChoose }) {
    this.coresForSystem = coresForSystem;
    this.currentCoreId = currentCoreId;
    this.onChoose = onChoose;

    this.root = document.createElement('div');
    this.root.className = 'coremenu';
    this.root.setAttribute('role', 'menu');
    this.root.hidden = true;

    // Two lines rather than one sentence: a long game name would otherwise ellipsise
    // away the "with…" and leave the menu unexplained.
    this.labelEl = document.createElement('p');
    this.labelEl.className = 'coremenu__label';
    this.labelEl.textContent = 'Play with…';

    this.titleEl = document.createElement('p');
    this.titleEl.className = 'coremenu__title';

    this.listEl = document.createElement('div');
    this.listEl.className = 'coremenu__list';

    this.root.append(this.labelEl, this.titleEl, this.listEl);
    document.body.appendChild(this.root);

    this.entryId = null;
    this._wire();
  }

  _wire() {
    this.listEl.addEventListener('click', (event) => {
      const button = event.target.closest('.coremenu__item');
      if (!button || !this.entryId) return;
      const entryId = this.entryId;
      const coreId = button.dataset.coreId;
      this.close();
      if (coreId) this.onChoose(entryId, coreId);
    });

    // Capture phase: a click that lands on a card would otherwise open the detail
    // sheet on the way past.
    this._onDocumentPointerDown = (event) => {
      if (this.root.hidden) return;
      if (!this.root.contains(event.target)) this.close();
    };
    document.addEventListener('pointerdown', this._onDocumentPointerDown, true);

    document.addEventListener('keydown', (event) => {
      if (this.root.hidden) return;
      if (event.key === 'Escape') {
        event.preventDefault();
        this.close();
        return;
      }
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        const items = [...this.listEl.querySelectorAll('.coremenu__item')];
        if (items.length === 0) return;
        const current = items.indexOf(document.activeElement);
        const next =
          event.key === 'ArrowDown'
            ? (current + 1) % items.length
            : (current - 1 + items.length) % items.length;
        items[next].focus();
      }
    });

    // A menu anchored to a card cannot follow it, so dismiss rather than let it
    // drift away from what it refers to.
    window.addEventListener('scroll', () => this.close(), { capture: true, passive: true });
    window.addEventListener('resize', () => this.close(), { passive: true });
  }

  /**
   * @param {object} opts
   * @param {string} opts.entryId
   * @param {string} opts.systemId
   * @param {string} opts.title game title, for the menu heading
   * @param {number} opts.x viewport coordinates of the pointer
   * @param {number} opts.y
   * @returns {boolean} false if there was nothing to choose between, in which case
   *   the menu stays closed and the caller should do nothing
   */
  open({ entryId, systemId, title, x, y }) {
    const candidates = this.coresForSystem(systemId);
    // One option is not a choice. Opening a single-item menu would train people to
    // long-press for nothing.
    if (candidates.length < 2) return false;

    this.entryId = entryId;
    this.titleEl.textContent = title;
    this.listEl.textContent = '';

    const active = this.currentCoreId(systemId);
    for (const core of candidates) {
      const item = document.createElement('button');
      item.type = 'button';
      item.className = 'coremenu__item';
      item.setAttribute('role', 'menuitem');
      item.dataset.coreId = core.id;
      if (core.id === active) item.dataset.active = 'true';

      const name = document.createElement('span');
      name.className = 'coremenu__name';
      name.textContent = core.name;

      const note = document.createElement('span');
      note.className = 'coremenu__note';
      // Saying which is which matters: choosing the placeholder gets you a
      // diagnostic pattern, and that should be a decision rather than a surprise.
      const kind = core.kind === 'libretro' ? 'real core' : 'diagnostic stand-in';
      note.textContent = core.id === active ? `${kind} · current` : kind;

      item.append(name, note);
      item.setAttribute(
        'aria-label',
        `${core.name}, ${kind}${core.id === active ? ', currently selected' : ''}`,
      );
      this.listEl.appendChild(item);
    }

    // Measured while visible but off-screen, so clamping uses the real size.
    this.root.hidden = false;
    this.root.style.left = '0px';
    this.root.style.top = '0px';
    const rect = this.root.getBoundingClientRect();
    const margin = 8;
    const left = Math.max(margin, Math.min(x, window.innerWidth - rect.width - margin));
    const top = Math.max(margin, Math.min(y, window.innerHeight - rect.height - margin));
    this.root.style.left = `${Math.round(left)}px`;
    this.root.style.top = `${Math.round(top)}px`;

    this.listEl.querySelector('.coremenu__item')?.focus({ preventScroll: true });
    return true;
  }

  close() {
    this.root.hidden = true;
    this.entryId = null;
  }

  get isOpen() {
    return !this.root.hidden;
  }
}

/**
 * Attaches right-click and long-press to a container of cards.
 *
 * Delegated from the container rather than bound per card: cards are recycled by the
 * virtualiser, so per-card listeners would have to be added and removed on every
 * scroll frame.
 *
 * @param {HTMLElement} container
 * @param {(detail: {card: HTMLElement, x: number, y: number}) => boolean} onRequest
 *   returns whether a menu actually opened, which decides if the following click is
 *   suppressed
 */
export function attachCoreMenuGestures(container, onRequest) {
  container.addEventListener('contextmenu', (event) => {
    const card = event.target.closest('.card');
    if (!card || card.hidden || card.dataset.locked === 'true') return;
    // Only swallow the browser menu if ours actually opened.
    if (onRequest({ card, x: event.clientX, y: event.clientY })) event.preventDefault();
  });

  let timer = 0;
  let origin = null;
  let opened = false;

  const cancel = () => {
    if (timer) clearTimeout(timer);
    timer = 0;
    origin = null;
  };

  container.addEventListener(
    'pointerdown',
    (event) => {
      // Long-press is a touch idiom; a held mouse button is a drag or a selection.
      if (event.pointerType === 'mouse') return;
      const card = event.target.closest('.card');
      if (!card || card.hidden || card.dataset.locked === 'true') return;
      origin = { x: event.clientX, y: event.clientY };
      opened = false;
      timer = setTimeout(() => {
        timer = 0;
        opened = onRequest({ card, x: origin.x, y: origin.y });
      }, LONG_PRESS_MS);
    },
    { passive: true },
  );

  container.addEventListener(
    'pointermove',
    (event) => {
      if (!timer || !origin) return;
      if (
        Math.abs(event.clientX - origin.x) > LONG_PRESS_SLOP ||
        Math.abs(event.clientY - origin.y) > LONG_PRESS_SLOP
      ) {
        cancel();
      }
    },
    { passive: true },
  );

  container.addEventListener('pointerup', cancel, { passive: true });
  container.addEventListener('pointercancel', cancel, { passive: true });

  // The click that ends a successful long-press must not also open the detail sheet.
  container.addEventListener(
    'click',
    (event) => {
      if (!opened) return;
      opened = false;
      event.stopPropagation();
      event.preventDefault();
    },
    true,
  );
}
