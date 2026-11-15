export type RevealStatus = 'playing' | 'paused' | 'complete';
export interface RevealFrame { status: RevealStatus; shown: number; total: number }

/** Presentation time only. This never changes source meshes or backend state. */
export class RevealTimeline {
  private status: RevealStatus = 'complete';
  private total = 0;
  private elapsed = 0;
  private startedAt = 0;
  private duration = 0;
  private hold = 0;

  start(total: number, now: number, duration: number, animate = true, hold = 0, allowEmpty = false): void {
    this.total = total;
    this.elapsed = 0;
    this.startedAt = now;
    this.duration = duration;
    this.hold = hold;
    this.status = animate && (total > 0 || allowEmpty) && duration > 0 ? 'playing' : 'complete';
  }

  read(now: number): RevealFrame {
    if (this.status === 'complete') return { status: 'complete', shown: this.total, total: this.total };
    const elapsed = this.elapsed + (this.status === 'playing' ? Math.max(0, now - this.startedAt) : 0);
    if (elapsed >= this.duration + this.hold) this.status = 'complete';
    return { status: this.status, shown: Math.min(this.total, Math.floor(this.total * elapsed / this.duration)), total: this.total };
  }

  pause(now: number): void {
    if (this.read(now).status !== 'playing') return;
    this.elapsed += Math.max(0, now - this.startedAt);
    this.status = 'paused';
  }

  resume(now: number): void {
    if (this.status !== 'paused') return;
    this.startedAt = now;
    this.status = 'playing';
  }

  skip(): void { this.status = 'complete'; }
}
