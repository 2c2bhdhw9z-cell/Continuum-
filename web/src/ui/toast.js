/**
 * Toasts. The only channel for transient feedback — core downloads, save states,
 * WebGPU failures.
 *
 * Bounded on purpose: a runaway error loop must not append hundreds of nodes to a
 * container the user cannot dismiss.
 */

const MAX_TOASTS = 4;
const DEFAULT_MS = 4200;

let container = null;

function ensureContainer() {
  if (!container) container = document.getElementById('toasts');
  return container;
}

/**
 * @param {string} title
 * @param {string} [body]
 * @param {{ kind?: 'info'|'warn'|'error', ms?: number }} [opts]
 */
export function toast(title, body = '', opts = {}) {
  const root = ensureContainer();
  if (!root) return;

  while (root.children.length >= MAX_TOASTS) root.firstElementChild?.remove();

  const el = document.createElement('div');
  el.className = 'toast';
  el.dataset.kind = opts.kind ?? 'info';

  const titleEl = document.createElement('p');
  titleEl.className = 'toast__title';
  titleEl.textContent = title;
  el.appendChild(titleEl);

  if (body) {
    const bodyEl = document.createElement('p');
    bodyEl.className = 'toast__body';
    bodyEl.textContent = body;
    el.appendChild(bodyEl);
  }

  root.appendChild(el);

  const ms = opts.ms ?? DEFAULT_MS;
  setTimeout(() => {
    el.classList.add('is-leaving');
    // Matches the CSS animation; removing sooner would cut it off mid-fade.
    setTimeout(() => el.remove(), 220);
  }, ms);

  return el;
}
