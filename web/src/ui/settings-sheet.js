/**
 * Settings: storage, cores, and the two toggles that are genuinely the user's call.
 *
 * Deliberately small. Three groups, and every row either reports a real measured
 * number or changes behaviour immediately — no "advanced" section, no settings whose
 * effect is invisible.
 *
 * ## Storage is counted, not estimated
 *
 * `navigator.storage.estimate()` is shown, but only as the quota line. It measures the
 * whole origin — the app shell, the service worker's caches, the core binaries — so
 * quoting it as "your ROMs" would tell someone with an empty library that they are
 * using twelve megabytes of it. The ROM and save-state figures are summed from their
 * metadata stores instead, which is also why payloads live in stores of their own:
 * adding up a hundred `size` fields reads kilobytes, where measuring the payloads
 * would deserialise every megabyte.
 *
 * ## Clearing is a two-step
 *
 * "Clear everything" destroys the user's ROM collection and every save state in one
 * click, and there is no undo because the bytes are gone. So the button arms first and
 * says exactly what will be destroyed, counted from storage rather than described
 * vaguely, and disarms itself if left alone.
 */

import { getSettings, setSetting, onSettingsChanged, applyTheme, THEMES } from '../data/settings.js';
import { storageBreakdown, clearEverything } from '../data/rom-store.js';
import { storageEstimate } from '../data/idb.js';
import { removeAllEntries } from '../data/catalog.js';
import { releaseAllUrls } from '../data/artwork.js';
import { clearAllMisses } from '../data/boxart.js';
import * as saveStates from '../data/save-states.js';
import { allCorePreferences, setCorePreference } from '../data/core-prefs.js';
import { phase1Systems } from '../data/systems.js';
import { toast } from './toast.js';

/** How long the destructive button stays armed before it forgets. */
const ARM_TIMEOUT_MS = 6000;

function formatMb(bytes) {
  if (!bytes) return '0 MB';
  const mb = bytes / 1048576;
  return mb < 0.1 ? `${(bytes / 1024).toFixed(0)} KB` : `${mb.toFixed(1)} MB`;
}

export class SettingsSheet {
  /**
   * @param {object} opts
   * @param {() => void} opts.onLibraryCleared
   * @param {() => void} opts.onSettingChanged
   * @param {(systemId: string) => {id: string, name: string, kind: string}[]} opts.coresForSystem
   */
  constructor({
    onLibraryCleared,
    onSettingChanged,
    coresForSystem = () => [],
    coresReady = Promise.resolve(),
  }) {
    this.onLibraryCleared = onLibraryCleared;
    this.onSettingChanged = onSettingChanged ?? (() => {});
    this.coresForSystem = coresForSystem;
    /** Settles once the core manifest has been declared. See `_buildCorePickers`. */
    this.coresReady = coresReady;

    this.root = document.getElementById('settings-sheet');
    this.romCountEl = document.getElementById('settings-rom-count');
    this.romBytesEl = document.getElementById('settings-rom-bytes');
    this.stateCountEl = document.getElementById('settings-state-count');
    this.stateBytesEl = document.getElementById('settings-state-bytes');
    this.artEl = document.getElementById('settings-art');
    this.quotaEl = document.getElementById('settings-quota');
    this.coresEl = document.getElementById('settings-cores');
    this.scaleSelect = document.getElementById('settings-scale');
    this.filterSelect = document.getElementById('settings-filter');
    this.themeSelect = document.getElementById('settings-theme');
    this.themeNoteEl = document.getElementById('settings-theme-note');
    this.hudToggle = document.getElementById('settings-hud');
    this.boxartToggle = document.getElementById('settings-boxart');
    this.captureToggle = document.getElementById('settings-capture');
    this.clearBtn = document.getElementById('settings-clear');
    this.clearNoteEl = document.getElementById('settings-clear-note');

    this._armed = false;
    this._armTimer = 0;

    this._wire();
    // Keep the checkboxes truthful if something else changes a setting.
    onSettingsChanged(() => this._syncToggles());
  }

  _wire() {
    for (const el of this.root.querySelectorAll('[data-close]')) {
      el.addEventListener('click', () => this.close());
    }
    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape' && !this.root.hidden) this.close();
    });

    this.hudToggle?.addEventListener('change', () => {
      setSetting('showHud', this.hudToggle.checked);
      this.onSettingChanged();
    });
    this.boxartToggle?.addEventListener('change', () => {
      setSetting('fetchBoxart', this.boxartToggle.checked);
      this.onSettingChanged();
    });
    this.captureToggle?.addEventListener('change', () => {
      setSetting('captureArtwork', this.captureToggle.checked);
      this.onSettingChanged();
    });

    // Themes are populated from the list the token layer actually defines, so the
    // dropdown cannot offer one that has no styles behind it.
    if (this.themeSelect) {
      for (const theme of THEMES) {
        const option = document.createElement('option');
        option.value = theme.id;
        option.textContent = theme.name;
        this.themeSelect.appendChild(option);
      }
      this.themeSelect.addEventListener('change', () => {
        setSetting('theme', this.themeSelect.value);
        applyTheme(this.themeSelect.value);
        this._syncDisplay();
        this.onSettingChanged();
      });
    }

    // Scaling and filtering are persisted defaults for what the player's own dropdowns
    // already changed per session; both are applied to a live session immediately.
    this.scaleSelect?.addEventListener('change', () => {
      setSetting('scaleMode', this.scaleSelect.value);
      this.onSettingChanged();
    });
    this.filterSelect?.addEventListener('change', () => {
      setSetting('filter', this.filterSelect.value);
      this.onSettingChanged();
    });

    this.clearBtn?.addEventListener('click', () => void this._handleClear());
  }

  // ------------------------------------------------------------- open / close

  async open() {
    this.root.hidden = false;
    this._disarm();
    this._syncToggles();
    this._buildCorePickers();
    // Rendered before the numbers arrive, so the sheet opens instantly rather than
    // waiting on three IndexedDB transactions.
    this._setStorageText('counting…');
    this.root.querySelector('.sheet__close')?.focus({ preventScroll: true });

    await this.refreshStorage();

    // The engine warms on idle, so opening Settings within the first second finds no
    // cores declared yet. Rebuilding once they are is the difference between the
    // picker being absent and the sheet stating there is nothing to choose.
    await this.coresReady;
    if (this.isOpen) this._buildCorePickers();
  }

  close() {
    this.root.hidden = true;
    this._disarm();
  }

  get isOpen() {
    return !this.root.hidden;
  }

  // -------------------------------------------------------------------- toggles

  _syncToggles() {
    const settings = getSettings();
    if (this.hudToggle) this.hudToggle.checked = settings.showHud;
    if (this.boxartToggle) this.boxartToggle.checked = settings.fetchBoxart;
    if (this.captureToggle) this.captureToggle.checked = settings.captureArtwork;
    this._syncDisplay();
  }

  _syncDisplay() {
    const settings = getSettings();
    if (this.scaleSelect) this.scaleSelect.value = settings.scaleMode;
    if (this.filterSelect) this.filterSelect.value = settings.filter;
    if (this.themeSelect) this.themeSelect.value = settings.theme;
    if (this.themeNoteEl) {
      this.themeNoteEl.textContent =
        THEMES.find((theme) => theme.id === settings.theme)?.note ?? '';
    }
  }

  // ---------------------------------------------------------------------- cores

  /**
   * One row per system that has a choice to make.
   *
   * Systems with a single core are omitted rather than shown disabled: a dropdown with
   * one option is not a setting, and eight of them would bury the two that matter.
   */
  _buildCorePickers() {
    if (!this.coresEl) return;
    this.coresEl.textContent = '';
    const stored = allCorePreferences();
    let rows = 0;

    for (const system of phase1Systems()) {
      const candidates = this.coresForSystem(system.id);
      if (candidates.length < 2) continue;
      rows++;

      const row = document.createElement('div');
      row.className = 'settings__row';

      const label = document.createElement('label');
      label.className = 'settings__label';
      label.textContent = system.name;
      label.htmlFor = `settings-core-${system.id}`;

      const select = document.createElement('select');
      select.id = `settings-core-${system.id}`;
      select.className = 'settings__select';

      const auto = document.createElement('option');
      auto.value = '';
      auto.textContent = `Default (${candidates[0].name})`;
      select.appendChild(auto);
      for (const core of candidates) {
        const option = document.createElement('option');
        option.value = core.id;
        option.textContent =
          core.kind === 'libretro' ? core.name : `${core.name} (diagnostic stand-in)`;
        select.appendChild(option);
      }
      // A stored id that is no longer offered falls back to the default, matching what
      // the Rust registry will actually do at launch.
      select.value = candidates.some((c) => c.id === stored[system.id]) ? stored[system.id] : '';

      select.addEventListener('change', () => {
        setCorePreference(system.id, select.value || null);
        toast(
          'Core preference saved',
          select.value
            ? `${system.name} will use ${candidates.find((c) => c.id === select.value)?.name}.`
            : `${system.name} is back to the default core.`,
        );
      });

      row.append(label, select);
      this.coresEl.appendChild(row);
    }

    if (rows === 0) {
      const note = document.createElement('p');
      note.className = 'settings__note';
      // Two very different situations, and saying the wrong one is misinformation.
      note.textContent = this._coresDeclared()
        ? 'Every system in this build has exactly one core, so there is nothing to choose yet.'
        : 'Reading the core manifest…';
      this.coresEl.appendChild(note);
    }
  }

  /** Whether any core has been declared yet. */
  _coresDeclared() {
    return phase1Systems().some((system) => this.coresForSystem(system.id).length > 0);
  }

  // -------------------------------------------------------------------- storage

  _setStorageText(text) {
    for (const el of [this.romBytesEl, this.stateBytesEl, this.quotaEl]) {
      if (el) el.textContent = text;
    }
  }

  /** Reads the real numbers out of storage and prints them. */
  async refreshStorage() {
    try {
      const [breakdown, estimate] = await Promise.all([storageBreakdown(), storageEstimate()]);
      this._breakdown = breakdown;

      if (this.romCountEl) {
        this.romCountEl.textContent =
          breakdown.romCount === 1 ? '1 ROM' : `${breakdown.romCount} ROMs`;
      }
      if (this.romBytesEl) this.romBytesEl.textContent = formatMb(breakdown.romBytes);
      if (this.stateCountEl) {
        this.stateCountEl.textContent =
          breakdown.stateCount === 1 ? '1 save state' : `${breakdown.stateCount} save states`;
      }
      if (this.stateBytesEl) this.stateBytesEl.textContent = formatMb(breakdown.stateBytes);
      if (this.artEl) {
        // Only blob-backed art occupies storage; a scraped URL is a string, and saying
        // "4 covers, 0 MB" without explaining that would look like a bug.
        this.artEl.textContent = breakdown.artCount
          ? `${breakdown.artCount} stored · ${formatMb(breakdown.artBytes)} of image data ` +
            '(archive covers are links, not copies)'
          : 'none stored yet';
      }
      if (this.quotaEl) {
        this.quotaEl.textContent = estimate
          ? `${formatMb(estimate.usage)} of ${formatMb(estimate.quota)} granted to this site ` +
            '(includes the app itself and the emulator cores)'
          : 'this browser does not report a storage quota';
      }
    } catch (err) {
      console.warn('[settings] could not read storage', err);
      this._setStorageText('unavailable');
    }
  }

  // ------------------------------------------------------------------- clearing

  _disarm() {
    this._armed = false;
    clearTimeout(this._armTimer);
    if (this.clearBtn) {
      this.clearBtn.textContent = 'Clear all stored ROMs and saves';
      this.clearBtn.classList.remove('is-armed');
    }
    if (this.clearNoteEl) {
      this.clearNoteEl.textContent =
        'Deletes every imported ROM, save state and stored cover from this browser. ' +
        'The four bundled test carts come back automatically.';
    }
  }

  async _handleClear() {
    const counts = this._breakdown ?? (await storageBreakdown());

    if (!this._armed) {
      if (counts.romCount === 0 && counts.stateCount === 0) {
        toast('Nothing to clear', 'There are no imported ROMs or save states stored.');
        return;
      }
      this._armed = true;
      this.clearBtn.textContent = 'Tap again to permanently delete';
      this.clearBtn.classList.add('is-armed');
      this.clearNoteEl.textContent =
        `This will delete ${counts.romCount} ROM${counts.romCount === 1 ? '' : 's'} ` +
        `(${formatMb(counts.romBytes)}) and ${counts.stateCount} save ` +
        `state${counts.stateCount === 1 ? '' : 's'} (${formatMb(counts.stateBytes)}). ` +
        'This cannot be undone.';
      // Disarms itself: a destructive button left armed while someone reads the rest of
      // the sheet is a trap waiting for a stray tap.
      this._armTimer = setTimeout(() => this._disarm(), ARM_TIMEOUT_MS);
      return;
    }

    try {
      const removed = await clearEverything();
      // In-memory state has to be torn down in the same breath, or the library keeps
      // showing cards whose bytes no longer exist.
      releaseAllUrls();
      removeAllEntries();
      saveStates.resetIndex();
      clearAllMisses();

      this._disarm();
      await this.refreshStorage();
      this.onLibraryCleared();
      toast(
        'Storage cleared',
        `Removed ${removed.roms} ROM${removed.roms === 1 ? '' : 's'} and ` +
          `${removed.states} save state${removed.states === 1 ? '' : 's'}.`,
      );
    } catch (err) {
      console.error('[settings] clear failed', err);
      toast('Could not clear storage', String(err?.message ?? err), { kind: 'error' });
    }
  }
}
