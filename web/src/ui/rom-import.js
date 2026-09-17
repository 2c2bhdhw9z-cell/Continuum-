/**
 * ROM import: file picker, drag-and-drop, and the library plumbing behind them.
 *
 * The flow is deliberately short, because this is the step between "an app" and "an
 * emulator you can use":
 *
 * ```text
 *   File → detect system from header → store in IndexedDB → add catalogue entry
 *        → refresh shelves → offer to play immediately
 * ```
 *
 * Detection reads headers rather than trusting extensions (see `rom-detect.js`), and
 * an unrecognised file is reported with what was actually found instead of being
 * silently filed under the wrong system.
 */

import { detectSystem, acceptedExtensions } from '../data/rom-detect.js';
import { putRom, romId, listRoms, deleteRom } from '../data/rom-store.js';
import { addRealEntry, removeEntry } from '../data/catalog.js';
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
    // they fire for every child element the pointer crosses, and a naive
    // show/hide flickers constantly.
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
      // One file, one obvious intention: start it.
      if (imported.length === 1) this.onPlay(first.id);
    }

    for (const reason of rejected.slice(0, 3)) {
      toast('Could not import', reason, { kind: 'warn', ms: 7000 });
    }
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

  /** Restores previously imported ROMs into the catalogue at boot. */
  async restoreLibrary() {
    let restored = 0;
    try {
      for (const meta of await listRoms()) {
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
    } catch (err) {
      // A blocked IndexedDB (private browsing, some embeds) must not stop the app;
      // the user simply cannot persist imports.
      console.warn('[import] could not restore library', err);
      return 0;
    }
    if (restored) {
      console.info(`[import] restored ${restored} imported ROM(s)`);
      this.onLibraryChanged();
    }
    return restored;
  }

  /** Deletes an imported ROM and its library entry. */
  async remove(entryId) {
    await deleteRom(entryId);
    removeEntry(entryId);
    this.onLibraryChanged();
    toast('Removed from library');
  }
}

/** "Super Mario Bros (USA).nes" → "Super Mario Bros (USA)". */
function titleFromFilename(filename) {
  const base = filename.replace(/\.[^.]+$/, '');
  return base.replace(/[_]+/g, ' ').trim() || filename;
}
