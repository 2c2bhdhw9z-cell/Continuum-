/**
 * AudioWorkletProcessor for emulator PCM.
 *
 * Runs on the audio render thread, which has a hard real-time deadline: every
 * `process()` call must return in well under the ~2.7 ms a 128-frame quantum
 * represents at 48 kHz. So this file allocates nothing after construction, does no
 * math beyond a copy, and never logs on the hot path.
 *
 * Deliberately self-contained (no `import`s): worklet module support for static
 * imports has been uneven across Safari versions, and a failed worklet load means
 * silence with no obvious cause.
 *
 * ## Buffer ownership
 *
 * The main thread transfers an `ArrayBuffer` of interleaved stereo samples, and
 * this processor transfers the emptied buffer straight back. That recycling is what
 * makes a per-frame audio hand-off allocation-free on both sides; without it, 60
 * buffers per second become garbage for the collector to chase.
 *
 * On underrun the output is padded with silence rather than repeating stale audio:
 * a short gap is far less noticeable than a click loop, and the underrun counter
 * makes the condition visible in the HUD.
 */

const CHANNELS = 2;

class PcmQueueProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const frames = options?.processorOptions?.ringFrames ?? 8192;
    this.capacity = frames * CHANNELS;
    this.ring = new Float32Array(this.capacity);
    this.head = 0;
    this.tail = 0;
    this.length = 0;

    this.underruns = 0;
    this.overruns = 0;
    this.statTick = 0;
    this.stopped = false;

    this.port.onmessage = (event) => this._onMessage(event);
  }

  _onMessage(event) {
    const data = event.data;
    if (!data) return;

    if (data.type === 'pcm') {
      const incoming = new Float32Array(data.buffer, 0, data.length);
      this._write(incoming);
      // Hand the storage back for reuse. Transferring makes it zero-copy.
      this.port.postMessage({ type: 'recycle', buffer: data.buffer }, [data.buffer]);
      return;
    }

    if (data.type === 'flush') {
      this.head = 0;
      this.tail = 0;
      this.length = 0;
      return;
    }

    if (data.type === 'stop') {
      this.stopped = true;
    }
  }

  _write(samples) {
    const cap = this.capacity;
    const count = Math.min(samples.length, cap);

    const first = Math.min(cap - this.tail, count);
    this.ring.set(samples.subarray(0, first), this.tail);
    const rest = count - first;
    if (rest > 0) this.ring.set(samples.subarray(first, count), 0);
    this.tail = (this.tail + count) % cap;

    const free = cap - this.length;
    if (count > free) {
      // Overwrote unread audio: the producer is ahead of the device.
      this.head = (this.head + (count - free)) % cap;
      this.length = cap;
      this.overruns++;
    } else {
      this.length += count;
    }
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    if (!output || output.length === 0) return !this.stopped;

    const left = output[0];
    const right = output.length > 1 ? output[1] : null;
    const quantum = left.length;

    const framesAvailable = (this.length / CHANNELS) | 0;
    const framesToWrite = Math.min(quantum, framesAvailable);

    let read = this.head;
    for (let i = 0; i < framesToWrite; i++) {
      left[i] = this.ring[read];
      if (right) right[i] = this.ring[read + 1];
      read += CHANNELS;
      if (read >= this.capacity) read = 0;
    }
    this.head = read;
    this.length -= framesToWrite * CHANNELS;

    if (framesToWrite < quantum) {
      // Silence, not repetition: a gap is quieter than a click.
      for (let i = framesToWrite; i < quantum; i++) {
        left[i] = 0;
        if (right) right[i] = 0;
      }
      this.underruns++;
    }

    // Report roughly every 250 ms. Frequent enough for a live HUD, rare enough to
    // stay off the real-time budget.
    if (++this.statTick >= 90) {
      this.statTick = 0;
      this.port.postMessage({
        type: 'stats',
        queuedFrames: (this.length / CHANNELS) | 0,
        capacityFrames: (this.capacity / CHANNELS) | 0,
        underruns: this.underruns,
        overruns: this.overruns,
      });
    }

    return !this.stopped;
  }
}

registerProcessor('pcm-queue', PcmQueueProcessor);
