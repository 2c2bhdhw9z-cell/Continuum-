/**
 * The Cheat Manager.
 *
 * Reachable from two places on purpose: the detail sheet, so a list can be prepared
 * before launching, and the player's controls, because deciding you want infinite lives
 * generally happens after you have died. Both open the same sheet against the same
 * stored list.
 *
 * ## Applying versus storing
 *
 * Storing is `data/cheats.js`. Applying is Rust, through `applyCheats`, and only while a
 * session is running. So the sheet always edits the stored list, and pushes to the core
 * only when there is one — which is why editing cheats for a game you are not playing is
 * perfectly legal and simply takes effect at launch.
 *
 * The push is whole-list, never incremental, because `retro_cheat_set` is indexed. See
 * `EmulatorBridge::apply_cheats`.
 *
 * ## Why there is no code validation
 *
 * Every system has its own convention, and cores accept several. Rejecting anything that
 * did not match a pattern would reject codes that work. The core is the authority: it
 * takes the string and either does something with it or does not.
 */

import * as cheats from '../data/cheats.js';
import { entryById } from '../data/catalog.js';
import { getSystem } from '../data/systems.js';
import { toast } from './toast.js';

/** Format hints, shown per system so the input is not a mystery box. */
const CODE_HINTS = {
  nes: 'Game Genie, e.g. SXIOPO — or an address:value pair like 00FF:09',
  snes: 'Game Genie, e.g. DD82-64DC — or Pro Action Replay, 7E0019:09',
  gb: 'Game Genie, e.g. 011-14D-E6E — or GameShark, 010114D6',
  gbc: 'Game Genie, e.g. 011-14D-E6E — or GameShark, 010114D6',
  gba: 'CodeBreaker or Action Replay, e.g. 8202A2E4 0100',
  sms: 'Address:value, e.g. 00C023:09',
  genesis: 'Game Genie, e.g. ACLA-AA3N — or address:value, FFB2C1:0009',
};

export class CheatSheet {
  /**
   * @param {object} opts
   * @param {import('../engine/bridge-host.js').BridgeHost} opts.host
   * @param {() => string|null} opts.runningGameId content id of the live session, if any
   * @param {() => void} [opts.onChanged]
   */
  constructor({ host, runningGameId, onChanged }) {
    this.host = host;
    this.runningGameId = runningGameId ?? (() => null);
    this.onChanged = onChanged ?? (() => {});

    this.root = document.getElementById('cheat-sheet');
    this.titleEl = document.getElementById('cheat-title');
    this.subtitleEl = document.getElementById('cheat-subtitle');
    this.listEl = document.getElementById('cheat-list');
    this.emptyEl = document.getElementById('cheat-empty');
    this.formEl = document.getElementById('cheat-form');
    this.descInput = document.getElementById('cheat-desc');
    this.codeInput = document.getElementById('cheat-code');
    this.hintEl = document.getElementById('cheat-hint');
    this.statusEl = document.getElementById('cheat-status');
    this.clearBtn = document.getElementById('cheat-clear');

    /** @type {string|null} */
    this.gameId = null;

    this._wire();
  }

  _wire() {
    for (const el of this.root.querySelectorAll('[data-close]')) {
      el.addEventListener('click', () => this.close());
    }
    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape' && !this.root.hidden) this.close();
    });

    this.formEl.addEventListener('submit', (event) => {
      event.preventDefault();
      void this._add();
    });

    // One delegated handler: rows come and go as the list is edited.
    this.listEl.addEventListener('click', (event) => {
      const row = event.target.closest('.cheat-row');
      if (!row || !this.gameId) return;
      const id = row.dataset.cheatId;

      if (event.target.closest('[data-action="delete"]')) {
        void this._remove(id);
      }
    });

    this.listEl.addEventListener('change', (event) => {
      const toggle = event.target.closest('input[type="checkbox"]');
      const row = event.target.closest('.cheat-row');
      if (!toggle || !row || !this.gameId) return;
      void this._toggle(row.dataset.cheatId, toggle.checked);
    });

    this.clearBtn.addEventListener('click', () => void this._clear());
  }

  // --------------------------------------------------------------- open / close

  /** @param {string} gameId */
  open(gameId) {
    const entry = entryById(gameId);
    if (!entry) return;
    this.gameId = gameId;

    const system = getSystem(entry.systemId);
    this.titleEl.textContent = 'Cheats';
    this.subtitleEl.textContent = `${entry.title} · ${system?.name ?? entry.systemId}`;
    this.hintEl.textContent =
      CODE_HINTS[entry.systemId] ?? 'Enter the code exactly as the cheat list gives it.';
    this.descInput.value = '';
    this.codeInput.value = '';

    this.root.hidden = false;
    this._render();
    this.codeInput.focus({ preventScroll: true });
  }

  close() {
    this.root.hidden = true;
    this.gameId = null;
    // Blanked on the way out, for the same reason the detail sheet is: nothing should
    // hold one game's data while a different game's sheet is opening.
    this.listEl.textContent = '';
    this.subtitleEl.textContent = '';
    this.statusEl.textContent = '';
    this.descInput.value = '';
    this.codeInput.value = '';
  }

  get isOpen() {
    return !this.root.hidden;
  }

  // ---------------------------------------------------------------- rendering

  /**
   * Rebuilds the list.
   *
   * Not virtualised, and that is a deliberate exception to rule 4: the list is capped at
   * 128 by `data/cheats.js`, it is not on a scroll path that matters, and a pooled
   * renderer here would be more machinery than the thing it renders.
   */
  _render() {
    if (!this.gameId) return;
    const list = cheats.listFor(this.gameId);

    this.listEl.textContent = '';
    for (const cheat of list) {
      const row = document.createElement('div');
      row.className = 'cheat-row';
      row.dataset.cheatId = cheat.id;

      const toggle = document.createElement('label');
      toggle.className = 'cheat-row__toggle';
      const box = document.createElement('input');
      box.type = 'checkbox';
      box.checked = cheat.enabled;
      box.setAttribute('aria-label', `Enable ${cheat.description || cheat.code}`);
      toggle.appendChild(box);

      const body = document.createElement('div');
      body.className = 'cheat-row__body';
      const name = document.createElement('p');
      name.className = 'cheat-row__name';
      name.textContent = cheat.description || '(no description)';
      const code = document.createElement('p');
      code.className = 'cheat-row__code';
      code.textContent = cheat.code;
      body.append(name, code);

      const del = document.createElement('button');
      del.type = 'button';
      del.className = 'textbtn cheat-row__delete';
      del.dataset.action = 'delete';
      del.textContent = 'Remove';
      del.setAttribute('aria-label', `Remove ${cheat.description || cheat.code}`);

      row.append(toggle, body, del);
      this.listEl.appendChild(row);
    }

    this.emptyEl.hidden = list.length > 0;
    this.clearBtn.hidden = list.length === 0;
    this._syncStatus();
  }

  /**
   * Says what is actually in force, which is not always what is in the list.
   *
   * Three genuinely different states: no session (stored, will apply at launch), a
   * session whose core takes cheats (applied now), and a session whose core does not.
   */
  _syncStatus() {
    if (!this.gameId) return;
    const total = cheats.countFor(this.gameId);
    const active = cheats.activeCountFor(this.gameId);
    const running = this.runningGameId() === this.gameId;

    if (!running) {
      this.statusEl.textContent = total
        ? `${active} of ${total} switched on · applied when you start the game`
        : '';
      return;
    }
    if (!this.host.bridge?.cheatsSupported) {
      this.statusEl.textContent = 'This core does not support cheats, so nothing is applied.';
      return;
    }
    const inCore = this.host.bridge.activeCheatCount;
    this.statusEl.textContent = `${active} of ${total} switched on · ${inCore} active in the running core`;
  }

  // ------------------------------------------------------------------ mutation

  /**
   * Pushes the stored list into the running core, if this game is the running one.
   *
   * Silent when there is no session: editing cheats for a game you are not playing is a
   * normal thing to do, and it takes effect at launch.
   */
  _pushToCore() {
    if (!this.gameId || this.runningGameId() !== this.gameId) return;
    const bridge = this.host.bridge;
    if (!bridge?.cheatsSupported) return;
    try {
      const { codes, enabled } = cheats.payloadFor(this.gameId);
      bridge.applyCheats(codes, enabled);
    } catch (err) {
      console.warn('[cheats] could not apply to the running core', err);
      toast('Cheats not applied', String(err?.message ?? err), { kind: 'warn' });
    }
  }

  async _add() {
    if (!this.gameId) return;
    try {
      const cheat = await cheats.add(this.gameId, {
        description: this.descInput.value,
        code: this.codeInput.value,
        enabled: true,
      });
      this.descInput.value = '';
      this.codeInput.value = '';
      this.codeInput.focus({ preventScroll: true });
      this._pushToCore();
      this._render();
      this.onChanged();
      toast('Cheat added', cheat.description || cheat.code);
    } catch (err) {
      toast('Could not add that cheat', String(err?.message ?? err), { kind: 'warn' });
    }
  }

  async _toggle(cheatId, enabled) {
    if (!this.gameId) return;
    await cheats.setEnabled(this.gameId, cheatId, enabled);
    this._pushToCore();
    this._render();
    this.onChanged();
  }

  async _remove(cheatId) {
    if (!this.gameId) return;
    await cheats.remove(this.gameId, cheatId);
    this._pushToCore();
    this._render();
    this.onChanged();
  }

  async _clear() {
    if (!this.gameId) return;
    const count = cheats.countFor(this.gameId);
    await cheats.clearFor(this.gameId);
    // Cleared in the core too, not merely forgotten here.
    if (this.runningGameId() === this.gameId && this.host.bridge?.cheatsSupported) {
      try {
        this.host.bridge.clearCheats();
      } catch (err) {
        console.warn('[cheats] could not clear in the core', err);
      }
    }
    this._render();
    this.onChanged();
    toast('Cheats cleared', `${count} removed for this game.`);
  }
}
