/**
 * Game card: the pooled node type used by every shelf and by the grid.
 *
 * Split into `createCard` (structure, once per pool slot) and `bindCard` (content, on
 * recycle) because that split is what makes recycling cheap. `bindCard` only writes
 * text and a couple of attributes — it never creates or removes elements, so a scroll
 * frame does no allocation and no layout invalidation beyond the node itself.
 *
 * The rule the virtualiser depends on: **binding must never change a card's size.**
 * Titles are line-clamped in CSS and the art box has a fixed height, so a long name
 * cannot push the row taller and desynchronise the scroller's maths.
 *
 * That rule is why cover art is an absolutely positioned `<img>` with `object-fit:
 * cover` rather than a background image on a sized element: a 512×512 boxart and a
 * 512×384 screenshot occupy exactly the same box, and an image that has not finished
 * decoding occupies it too. Nothing about the artwork can move the layout, which is
 * also what lets art appear asynchronously — the scraper resolving a cover ten seconds
 * after the shelf rendered just swaps a `src`.
 */

import { artFor, glyphFor, subtitleFor } from './art.js';
import { entryAt } from '../data/catalog.js';
import { displayUrlFor } from '../data/artwork.js';
import { autoStateFor } from '../data/save-states.js';
import { getSystem } from '../data/systems.js';

/** Builds an empty card. Called `poolSize` times per scroller, then never again. */
export function createCard() {
  const card = document.createElement('button');
  card.type = 'button';
  card.className = 'card';

  const art = document.createElement('div');
  art.className = 'card__art';

  // Cover art, over the generated plate. `alt` is empty because the card's own
  // aria-label already names the game; announcing the title twice is worse than not
  // describing the image.
  const image = document.createElement('img');
  image.className = 'card__img';
  image.alt = '';
  image.decoding = 'async';
  image.loading = 'lazy';
  image.hidden = true;
  // A cover that 404s later (the archive is rewritten from time to time) must fall
  // back to the plate rather than leaving a broken-image icon on the shelf.
  image.addEventListener('error', () => {
    image.hidden = true;
  });
  image.addEventListener('load', () => {
    image.hidden = false;
  });

  const badge = document.createElement('span');
  badge.className = 'card__badge';

  const glyph = document.createElement('span');
  glyph.className = 'card__glyph';

  // SVG rather than a "★" text glyph: the star is missing from some system font
  // stacks (and from most headless font sets), where it renders as tofu.
  const fav = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  fav.setAttribute('class', 'card__fav');
  fav.setAttribute('viewBox', '0 0 24 24');
  fav.setAttribute('aria-hidden', 'true');
  const favPath = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  favPath.setAttribute(
    'd',
    'M12 2.6l2.9 6.1 6.6.9-4.8 4.6 1.2 6.6L12 17.6 6.1 20.8l1.2-6.6L2.5 9.6l6.6-.9z',
  );
  fav.appendChild(favPath);
  fav.style.display = 'none';

  // "RESUME", when this game has an auto-save waiting. This replaced a percentage
  // progress bar, which was reporting a number no emulator can know: there is no way
  // to tell how far through a game a save state is. Whether launching will drop you
  // back where you left off is both knowable and worth knowing.
  const resume = document.createElement('span');
  resume.className = 'card__resume';
  resume.textContent = 'RESUME';
  resume.hidden = true;

  art.append(image, badge, glyph, fav, resume);

  const label = document.createElement('span');
  label.className = 'card__label';
  const title = document.createElement('p');
  title.className = 'card__title';
  const sub = document.createElement('p');
  sub.className = 'card__sub';
  label.append(title, sub);

  card.append(art, label);

  // Cached references: `bindCard` runs on the scroll path, and querySelector there
  // would be a per-frame DOM walk for no reason.
  card._refs = { art, image, badge, glyph, fav, resume, title, sub };
  return card;
}

/**
 * Populates a card from a library index.
 * @param {HTMLElement} card
 * @param {number} catalogIndex Index into the library, not the visible list.
 */
export function bindCard(card, catalogIndex) {
  const entry = entryAt(catalogIndex);
  const r = card._refs;
  if (!entry) {
    card.hidden = true;
    return;
  }
  card.hidden = false;

  // Identity for click handling and for tests, so a card always knows what it is.
  card.dataset.entryId = entry.id;
  card.dataset.catalogIndex = String(catalogIndex);

  const system = getSystem(entry.systemId);
  r.art.style.setProperty('--art', artFor(entry));
  r.badge.textContent = system?.short ?? entry.systemId;
  r.glyph.textContent = glyphFor(entry);
  r.title.textContent = entry.title;
  r.sub.textContent = subtitleFor(entry);

  const artUrl = displayUrlFor(entry);
  if (artUrl) {
    // Only touch `src` when it actually changes: reassigning the same URL restarts the
    // decode, and on a shelf being scrolled back and forth that is real work for an
    // identical result.
    if (r.image.getAttribute('src') !== artUrl) {
      r.image.hidden = true;
      r.image.setAttribute('src', artUrl);
    } else if (r.image.complete && r.image.naturalWidth > 0) {
      // Already decoded from a previous binding of this same node.
      r.image.hidden = false;
    }
    card.dataset.art = entry.art?.tier ?? 'yes';
  } else {
    r.image.hidden = true;
    // `removeAttribute`, not `src = ''`: an empty src resolves to the page URL and
    // fires a pointless request for the document.
    r.image.removeAttribute('src');
    delete card.dataset.art;
  }

  // `hidden` is unreliable on SVG elements in some engines; toggle display.
  r.fav.style.display = entry.favorite ? 'block' : 'none';
  // A synchronous Map lookup once the state index is hydrated, so it is safe here.
  r.resume.hidden = !autoStateFor(entry.id);

  // Phase 2 systems cannot be imported, so a locked card can only exist if a future
  // build starts listing them again; the guard costs nothing and keeps the launch
  // path's contract visible.
  const locked = system?.phase === 2;
  card.dataset.locked = locked ? 'true' : 'false';
  card.setAttribute(
    'aria-label',
    locked
      ? `${entry.title} — ${system.name}, available in the native app`
      : `Play ${entry.title} — ${subtitleFor(entry)}`,
  );
}
