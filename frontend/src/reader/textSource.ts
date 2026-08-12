// Chunked txt content loader: the server streams 6000-char slices, we cache
// them (client also applies ETag caching) and expose sync slicing.

import { bookApi } from '../api';

export const TXT_CHUNK_SIZE = 6000;

interface Chunk {
  /** server-reported char offset right after this chunk */
  next: number;
  text: string;
}

export class TextSource {
  private chunks = new Map<number, Chunk>();
  private loader: (
    position: number,
    length: number,
  ) => Promise<{ content: string; next_position: number }>;
  private pending = new Map<number, Promise<void>>();

  constructor(
    bookId: number,
    readonly totalChars: number,
    loader?: (
      position: number,
      length: number,
    ) => Promise<{ content: string; next_position: number }>,
  ) {
    this.loader =
      loader ??
      ((position, length) => bookApi.content(bookId, position, length));
  }

  chunkStart(offset: number): number {
    return Math.floor(offset / TXT_CHUNK_SIZE) * TXT_CHUNK_SIZE;
  }

  /** Load every chunk intersecting [start, end). */
  async ensureRange(start: number, end: number): Promise<void> {
    const jobs: Promise<void>[] = [];
    for (
      let at = this.chunkStart(start);
      at < end && at < this.totalChars;
      at += TXT_CHUNK_SIZE
    ) {
      if (this.chunks.has(at)) continue;
      jobs.push(this.load(at));
    }
    await Promise.all(jobs);
  }

  has(offset: number): boolean {
    return this.chunks.has(this.chunkStart(offset));
  }

  /** Sync slice; caller must ensureRange first. Tolerates partial coverage. */
  slice(start: number, end: number): string {
    let out = '';
    for (
      let at = this.chunkStart(start);
      at < end && at < this.totalChars;
      at += TXT_CHUNK_SIZE
    ) {
      const chunk = this.chunks.get(at);
      if (!chunk) break;
      const from = Math.max(start - at, 0);
      const to = Math.min(end - at, chunk.text.length);
      out += chunk.text.slice(from, to);
    }
    return out;
  }

  private load(at: number): Promise<void> {
    const existing = this.pending.get(at);
    if (existing) return existing;
    const job = this.loader(at, TXT_CHUNK_SIZE)
      .then(res => {
        this.chunks.set(at, { next: res.next_position, text: res.content });
      })
      .finally(() => {
        this.pending.delete(at);
      });
    this.pending.set(at, job);
    return job;
  }
}
