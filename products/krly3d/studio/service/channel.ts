// Backpressure between graph nodes and the RPC iterator. Large meshes never
// accumulate in LangGraph state or an unbounded custom-event queue.
export class Channel<T> {
  private items: T[] = [];
  private readable?: () => void;
  private writable?: () => void;
  private ended = false;
  private failure?: unknown;
  constructor(private capacity = 2) {}
  async send(item: T) {
    while (this.items.length >= this.capacity && !this.ended) {
      await new Promise<void>((r) => this.writable = r);
    }
    if (this.ended) {
      throw this.failure ??
        new DOMException("Operation cancelled", "AbortError");
    }
    this.items.push(item);
    this.readable?.();
    this.readable = undefined;
  }
  close(failure?: unknown) {
    this.ended = true;
    this.failure = failure;
    this.readable?.();
    this.writable?.();
  }
  async *[Symbol.asyncIterator]() {
    for (;;) {
      if (this.items.length) {
        const item = this.items.shift()!;
        this.writable?.();
        this.writable = undefined;
        yield item;
      } else if (this.ended) {
        if (this.failure) throw this.failure;
        return;
      } else await new Promise<void>((r) => this.readable = r);
    }
  }
}
