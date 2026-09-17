/**
 * ROM import: file picker, drag-and-drop, and the library plumbing behind them.
 *
 * The flow is deliberately short, because this is the step between "an app" and "an
 * emulator you can use":
 *
 * ```text
 *   File → detect system from header → store bytes in IndexedDB → add library entry
 *        → refresh shelves → offer to play immediately
 *        → (in the background) look for cover art
 * ```
 *
 * Detection reads headers rather than trusting extensions (see `rom-detect.js`), and an
 * unrecognised file is reported with what was actually found instead of being silently
 * filed under the wrong system.
 *
 * ## Import once
 *
 * The bytes go to IndexedDB, keyed by a hash of their content, and the library is
 * populated from that store at every boot. Importing is therefore a one-time act: close
 * the tab, close the app, restart the phone, and the collection is still there. Nothing
 * is uploaded anywhere — `restoreLibrary` reading the same rows back is the entire
 * persistence mechanism.
 */

import { detectSystem, acceptedExtensions } from '../data/rom-detect.js';
import { putRom, romId, listRoms, listFlags, deleteRom } from '../data/rom-store.js';
import { addRealEntry, removeEntry, applyStoredFlags } from '../data/catalog.js';
import { hydrateArtwork, scrapeArtwork, clearArtwork } from '../data/artwork.js';
import { getSetting } from '../data/settings.js';
import { stripDumpTags } from '../data/boxart.js';
import { getSystem } from '../data/systems.js';
import { toast } from './toast.js';

/** Refuse absurd inputs early: a 512 MB "NES ROM" is a mistake, not a ROM. */
const MAX_BYTES = 512 * 1024 * 1024;

export class RomImporter {
  /**
   * @param {object} opts
   * @param {() => void} opts.onLibraryChanged
   * @param {(entryId: string) => void} opts.onPlay
   */
  constructor({ onLibraryChanged, onPlay }) {
    this.onLibraryChanged = onLibraryChanged;
    this.onPlay = onPlay;

    this.input = document.getElementById('rom-file-input');
    this.dropZone = document.getElementById('app');
    this.dropHint = document.getElementById('drop-hint');
    this._dragDepth = 0;

    this._wire();
  }

  _wire() {
    if (this.input) {
      this.input.accept = acceptedExtensions().join(',');
      this.input.addEventListener('change', () => {
        const files = Array.from(this.input.files ?? []);
        // Reset first: picking the same file twice must fire `change` again.
        this.input.value = '';
        void this.importFiles(files);
      });
    }

    for (const button of document.querySelectorAll('[data-action="import-rom"]')) {
      button.addEventListener('click', () => this.openPicker());
    }

    // Drag-and-drop over the whole app. `dragenter`/`dragleave` are counted because
    // they fire for every child element the pointer crosses, and a naive show/hide
    // flickers constantly.
    this.dropZone.addEventListener('dragenter', (event) => {
      if (!this._hasFiles(event)) return;
      event.preventDefault();
      this._dragDepth++;
      this.dropHint?.removeAttribute('hidden');
    });
    this.dropZone.addEventListener('dragover', (event) => {
      if (!this._hasFiles(event)) return;
      event.preventDefault();
      event.dataTransfer.dropEffect = 'copy';
    });
    this.dropZone.addEventListener('dragleave', (event) => {
      if (!this._hasFiles(event)) return;
      this._dragDepth = Math.max(0, this._dragDepth - 1);
      if (this._dragDepth === 0) this.dropHint?.setAttribute('hidden', '');
    });
    this.dropZone.addEventListener('drop', (event) => {
      if (!this._hasFiles(event)) return;
      event.preventDefault();
      this._dragDepth = 0;
      this.dropHint?.setAttribute('hidden', '');
      void this.importFiles(Array.from(event.dataTransfer.files ?? []));
    });
  }

  _hasFiles(event) {
    return Array.from(event.dataTransfer?.types ?? []).includes('Files');
  }

  openPicker() {
    this.input?.click();
  }

  /**
   * Imports files, reporting per-file outcomes.
   * @param {File[]} files
   */
  async importFiles(files) {
    if (!files.length) return;

    const imported = [];
    const rejected = [];

    for (const file of files) {
      try {
        const entry = await this.importFile(file);
        imported.push(entry);
      } catch (err) {
        console.warn('[import] rejected', file.name, err);
        rejected.push(`${file.name}: ${err.message ?? err}`);
      }
    }

    if (imported.length) {
      this.onLibraryChanged();
      const first = imported[0];
      const system = getSystem(first.systemId);
      toast(
        imported.length === 1 ? `Added ${first.title}` : `Added ${imported.length} ROMs`,
        imported.length === 1
          ? `${system?.name ?? first.systemId} · press Play to start`
          : imported.map((e) => e.title).slice(0, 4).join(', '),
      );
      // Artwork is looked for after the library has already updated, so a card appears
      // instantly with its generated plate and gains a cover when one is found. Making
      // the import wait on six network probes per file would turn a local operation
      // into a slow one for a purely cosmetic result.
      void this.fetchArtwork(imported);
      // One file, one obvious intention: start it.
      if (imported.length === 1) this.onPlay(first.id);
    }

    for (const reason of rejected.slice(0, 3)) {
      toast('Could not import', reason, { kind: 'warn', ms: 7000 });
    }
  }

  /**
   * Resolves cover art for a batch, refreshing the library as each one lands.
   *
   * Sequential rather than `Promise.all`: `boxart.js` already limits concurrency, and
   * refreshing the shelves once per resolved cover keeps the UI responsive instead of
   * doing nothing for ten seconds and then reflowing everything at once.
   */
  async fetchArtwork(entries) {
    if (!getSetting('fetchBoxart')) return 0;
    let found = 0;
    for (const entry of entries) {
      try {
        if (await scrapeArtwork(entry)) {
          found++;
          this.onLibraryChanged();
        }
      } catch (err) {
        console.warn('[import] artwork lookup failed for', entry.id, err);
      }
    }
    if (found) console.info(`[artwork] resolved ${found} of ${entries.length} cover(s)`);
    return found;
  }

  /**
   * @param {File} file
   * @returns {Promise<import('../data/catalog.js').CatalogEntry>}
   */
  async importFile(file) {
    if (file.size === 0) throw new Error('the file is empty');
    if (file.size > MAX_BYTES) {
      throw new Error(`${(file.size / 1024 / 1024).toFixed(0)} MB exceeds the import limit`);
    }

    const bytes = new Uint8Array(await file.arrayBuffer());
    const detection = detectSystem(bytes, file.name);

    if (!detection.systemId) {
      throw new Error(detection.detail);
    }
    const system = getSystem(detection.systemId);
    if (!system) {
      throw new Error(`detected ${detection.systemId}, which this build does not support`);
    }
    if (system.phase === 2) {
      throw new Error(`${system.name} content needs the native build (Phase 2)`);
    }

    const id = romId(bytes, detection.extension);
    const meta = await putRom({
      id,
      name: file.name,
      systemId: detection.systemId,
      extension: detection.extension,
      bytes,
    });

    console.info(
      `[import] ${file.name} → ${detection.systemId} (${detection.confidence}: ${detection.detail})`,
    );

    return addRealEntry({
      id,
      title: titleFromFilename(file.name),
      systemId: detection.systemId,
      sizeBytes: bytes.length,
      filename: file.name,
      source: 'imported',
      addedAt: meta.addedAt,
      blurb: `Imported ${file.name} · ${detection.detail}.`,
    });
  }

  /**
   * Rebuilds the library from IndexedDB at boot.
   *
   * This is the whole of "import once": three `getAll()` calls — the ROM metadata, the
   * user's flags, the artwork — and the collection is back exactly as it was left,
   * including favourites and which shelf each game belongs to. Payloads are not read
   * here; a ROM's bytes are fetched only when it is launched, so restoring a hundred
   * games costs kilobytes rather than gigabytes.
   *
   * @returns {Promise<number>} how many entries were restored
   */
  async restoreLibrary() {
    let restored = 0;
    try {
      for (const meta of await listRoms()) {
        // A ROM stored by a build that knew a system this one does not would otherwise
        // become an entry that can never launch.
        if (!getSystem(meta.systemId)) continue;
        addRealEntry({
          id: meta.id,
          title: titleFromFilename(meta.name),
          systemId: meta.systemId,
          sizeBytes: meta.size,
          filename: meta.name,
          source: 'imported',
          addedAt: meta.addedAt,
        });
        restored++;
      }

      // Applied after the entries exist, and to built-ins as well as imports — the
      // flags store is keyed by entry id, not by ROM.
      for (const flags of await listFlags()) applyStoredFlags(flags);
      const withArt = await hydrateArtwork();

      if (restored || withArt) {
        console.info(`[import] restored ${restored} ROM(s), ${withArt} with artwork`);
      }
    } catch (err) {
      // A blocked IndexedDB (private browsing, some embeds) must not stop the app; the
      // user simply cannot persist imports.
      console.warn('[import] could not restore library', err);
      return 0;
    }
    this.onLibraryChanged();
    return restored;
  }

  /**
   * Looks for artwork for anything in the library that still has none.
   *
   * Runs once on idle after boot, which covers two cases the import path cannot: a ROM
   * imported while the setting was off, and one whose cover was added to the archive
   * after it was imported. The negative cache is what stops this from being six network
   * probes per artless game on every single launch.
   */
  async backfillArtwork(entries) {
    if (!getSetting('fetchBoxart')) return 0;
    const pending = entries.filter((entry) => !entry.art && entry.source === 'imported');
    if (!pending.length) return 0;
    return this.fetchArtwork(pending);
  }

  /** Deletes an imported ROM, its states, its artwork and its library entry. */
  async remove(entryId) {
    await deleteRom(entryId);
    await clearArtwork(entryId);
    removeEntry(entryId);
    this.onLibraryChanged();
    toast('Removed from library');
  }
}

/**
 * "Super Mario World (USA) [!].sfc" → "Super Mario World (USA)".
 *
 * The region tag stays, because it is real information about which dump this is. The
 * square-bracket tags go: `[!]`, `[b1]`, `[T+Eng]` describe the dump's provenance to a
 * cataloguing tool, not the game to a person, and leaving them in put "[!]" on the shelf.
 *
 * Only the *title* is trimmed. Cover art is still looked up from the original filename,
 * which is what the archive's own naming is derived from.
 */
function titleFromFilename(filename) {
  const base = filename.replace(/\.[^.]+$/, '');
  return (
    stripDumpTags(base.replace(/[_]+/g, ' ')).trim() || base.trim() || filename
  );
}
