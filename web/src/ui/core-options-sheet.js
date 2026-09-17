/**
 * Core options: the settings the emulator core itself declares.
 *
 * These are not application settings, which is why they are not in the Settings sheet.
 * The list is different for every core — mGBA offers colour correction and a frameskip,
 * Snes9x offers overscan cropping and layer toggles — and it is published by the core at
 * instantiation, so it can only be shown once a core is loaded. Opened from the player's
 * controls while a game is running.
 *
 * ## Where a value lives
 *
 * Three places, and the layering is deliberate:
 *
 *   1. **The core**, which read it through `GET_VARIABLE` and will re-read it when told
 *      the values changed. Set through Rust.
 *   2. **`data/core-options.js`**, in localStorage, so the choice outlives the session.
 *   3. **Nowhere else.** The list of *available* options is never stored — it is asked of
 *      the core every time, so a core that gains or renames an option cannot leave a
 *      stale dropdown behind.
 *
 * ## Restart, or not
 *
 * Most options take effect immediately: the core polls
 * `RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE` and re-reads what changed. Some are only
 * consulted while content is loading, and only the core knows which — so rather than
 * guess, the sheet says plainly that a few need the game restarted.
 */

import * as coreOptions from '../data/core-options.js';
import { toast } from './toast.js';

export class CoreOptionsSheet {
  /**
   * @param {object} opts
   * @param {import('../engine/bridge-host.js').BridgeHost} opts.host
   * @param {() => string|null} opts.runningCoreId
   * @param {() => string} opts.runningCoreName
   */
  constructor({ host, runningCoreId, runningCoreName }) {
    this.host = host;
    this.runningCoreId = runningCoreId ?? (() => null);
    this.runningCoreName = runningCoreName ?? (() => 'the core');

    this.root = document.getElementById('coreopts-sheet');
    this.subtitleEl = document.getElementById('coreopts-subtitle');
    this.listEl = document.getElementById('coreopts-list');
    this.emptyEl = document.getElementById('coreopts-empty');
    this.resetBtn = document.getElementById('coreopts-reset');

    this._wire();
  }

  _wire() {
    for (const el of this.root.querySelectorAll('[data-close]')) {
      el.addEventListener('click', () => this.close());
    }
    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape' && !this.root.hidden) this.close();
    });
    this.resetBtn.addEventListener('click', () => this._resetAll());
  }

  /**
   * Reads the option table out of the running core, through Rust.
   *
   * Flat groups of four — `[key, label, value, choices]` — which is how the list crosses
   * both the core→JS and Rust→JS boundaries. See `WasmEmulatorBridge::core_options_flat`.
   */
  _read() {
    const flat = this.host.bridge?.coreOptionsFlat?.() ?? [];
    const out = [];
    for (let i = 0; i + 3 < flat.length; i += 4) {
      out.push({
        key: flat[i],
        label: flat[i + 1],
        value: flat[i + 2],
        values: flat[i + 3].split('|').filter(Boolean),
      });
    }
    return out;
  }

  open() {
    this.root.hidden = false;
    this._render();
  }

  close() {
    this.root.hidden = true;
    this.listEl.textContent = '';
    this.subtitleEl.textContent = '';
  }

  get isOpen() {
    return !this.root.hidden;
  }

  _render() {
    const coreId = this.runningCoreId();
    const options = this._read();
    this.subtitleEl.textContent = coreId
      ? `${this.runningCoreName()} · ${options.length} option${options.length === 1 ? '' : 's'}`
      : 'No core is running.';

    this.listEl.textContent = '';
    const stored = coreId ? coreOptions.valuesFor(coreId) : {};

    for (const option of options) {
      const row = document.createElement('div');
      row.className = 'settings__row';

      const label = document.createElement('label');
      label.className = 'settings__label';
      label.textContent = option.label;
      label.htmlFor = `coreopt-${option.key}`;

      const select = document.createElement('select');
      select.className = 'settings__select';
      select.id = `coreopt-${option.key}`;
      select.dataset.key = option.key;

      // "Core default" is a real choice, distinct from picking the value that happens to
      // be the default today: it means "stay unset", so a core that changes its default
      // in a later build takes the new one.
      const auto = document.createElement('option');
      auto.value = '';
      auto.textContent = `Core default (${option.values[0] ?? '—'})`;
      select.appendChild(auto);

      for (const value of option.values) {
        const el = document.createElement('option');
        el.value = value;
        el.textContent = value;
        select.appendChild(el);
      }
      select.value = option.values.includes(stored[option.key]) ? stored[option.key] : '';

      select.addEventListener('change', () => this._apply(option, select.value));

      row.append(label, select);
      this.listEl.appendChild(row);
    }

    this.emptyEl.hidden = options.length > 0;
    this.resetBtn.hidden = options.length === 0 || Object.keys(stored).length === 0;
  }

  _apply(option, chosen) {
    const coreId = this.runningCoreId();
    if (!coreId) return;
    // Empty means "unset": store nothing and hand the core its own first choice back, so
    // the change takes effect now rather than at the next launch.
    const value = chosen || option.values[0] || '';
    try {
      this.host.bridge.setCoreOption(option.key, value);
      coreOptions.set(coreId, option.key, chosen || null);
      this._render();
      toast(option.label, chosen ? `Set to ${chosen}` : 'Back to the core default', { ms: 3500 });
    } catch (err) {
      console.warn('[core-options] could not set', option.key, err);
      toast('Could not change that option', String(err?.message ?? err), { kind: 'warn' });
    }
  }

  _resetAll() {
    const coreId = this.runningCoreId();
    if (!coreId) return;
    const options = this._read();
    coreOptions.clearFor(coreId);
    for (const option of options) {
      try {
        this.host.bridge.setCoreOption(option.key, option.values[0] ?? '');
      } catch (err) {
        console.warn('[core-options] could not reset', option.key, err);
      }
    }
    this._render();
    toast('Core options reset', `${this.runningCoreName()} is back to its defaults.`);
  }
}
