/**
 * Game card: the pooled node type used by every shelf and by the grid.
 *
 * Split into `createCard` (structure, once per pool slot) and `bindCard` (content,
 * on recycle) because that split is what makes recycling cheap. `bindCard` only
 * writes text and a couple of style properties — it never creates or removes
 * elements, so a scroll frame does no allocation and no layout invalidation beyond
 * the node itself.
 *
 * The rule the virtualiser depends on: **binding must never change a card's size.**
 * Titles are line-clamped in CSS and the art box has a fixed aspect, so a long name
 * cannot push the row taller and desynchronise the scroller's maths.
 */

import { artFor, glyphFor, subtitleFor } from './art.js';
import { entryAt } from '../data/catalog.js';
import { getSystem } from '../data/systems.js';

/** Builds an empty card. Called `poolSize` times per scroller, then never again. */
export function createCard() {
  const card = document.createElement('button');
  card.type = 'button';
  card.className = 'card';

  const art = document.createElement('div');
  art.className = 'card__art';

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

  const progress = document.createElement('span');
  progress.className = 'card__progress';
  const progressBar = document.createElement('i');
  progress.appendChild(progressBar);
  progress.hidden = true;

  art.append(badge, glyph, fav, progress);

  const label = document.createElement('span');
  label.className = 'card__label';
  const title = document.createElement('p');
  title.className = 'card__title';
  const sub = document.createElement('p');
  sub.className = 'card__sub';
  label.append(title, sub);

  card.append(art, label);

  // Cached references: `bindCard` runs on the scroll path, and querySelector
  // there would be a per-frame DOM walk for no reason.
  card._refs = { art, badge, glyph, fav, progress, progressBar, title, sub };
  return card;
}

/**
 * Populates a card from a catalogue index.
 * @param {HTMLElement} card
 * @param {number} catalogIndex Index into the catalogue, not the visible list.
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

  // `hidden` is unreliable on SVG elements in some engines; toggle display.
  r.fav.style.display = entry.favorite ? 'block' : 'none';

  if (entry.progress > 0) {
    r.progress.hidden = false;
    r.progressBar.style.width = `${Math.round(entry.progress * 100)}%`;
  } else {
    r.progress.hidden = true;
  }

  // Phase 2 systems are visible but not launchable in the browser build.
  const locked = system?.phase === 2;
  card.dataset.locked = locked ? 'true' : 'false';
  card.setAttribute(
    'aria-label',
    locked
      ? `${entry.title} — ${system.name}, available in the native app`
      : `Play ${entry.title} — ${subtitleFor(entry)}`,
  );
}
