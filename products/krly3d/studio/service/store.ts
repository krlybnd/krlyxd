import { Code, ConnectError } from "@connectrpc/connect";
import type { PendingQuestion } from "./questions.ts";

export type BlobEntry = {
  id: string;
  name: string;
  mime: string;
  bytes: Uint8Array;
  created: number;
};
const TTL = 60 * 60 * 1000;
const MAX_TOTAL = 256 * 1024 * 1024;
export class BlobStore {
  private entries = new Map<string, BlobEntry>();
  private total = 0;
  private prune() {
    for (const [id, item] of this.entries) {
      if (Date.now() - item.created > TTL) {
        this.entries.delete(id);
        this.total -= item.bytes.length;
      }
    }
  }
  put(name: string, mime: string, bytes: Uint8Array): BlobEntry {
    this.prune();
    if (
      bytes.length > 64 * 1024 * 1024 ||
      this.total + bytes.length > MAX_TOTAL || this.entries.size >= 256
    ) {
      throw new ConnectError(
        "A munkatér tárhelykerete megtelt. Mentsd le az eredményeket; a régi fájlok egy óra után lejárnak.",
        Code.ResourceExhausted,
      );
    }
    const entry = {
      id: crypto.randomUUID(),
      name: name.replace(/[^\p{L}\p{N}._ -]/gu, "_").slice(0, 120) ||
        "artifact",
      mime,
      bytes,
      created: Date.now(),
    };
    this.entries.set(entry.id, entry);
    this.total += bytes.length;
    return entry;
  }
  get(id: string): BlobEntry {
    this.prune();
    const item = this.entries.get(id);
    if (!item) {
      throw new ConnectError(
        "A fájl nem található vagy lejárt. Töltsd fel újra.",
        Code.NotFound,
      );
    }
    return item;
  }
}

export function imageFormat(bytes: Uint8Array): "png" | "jpeg" | "pnm" {
  if (bytes.length < 8 || bytes.length > 16 * 1024 * 1024) {
    throw new ConnectError(
      "A kép mérete 8 byte és 16 MiB között lehet.",
      Code.InvalidArgument,
    );
  }
  if ([137, 80, 78, 71, 13, 10, 26, 10].every((v, i) => bytes[i] === v)) {
    return "png";
  }
  if (bytes[0] === 255 && bytes[1] === 216 && bytes[2] === 255) return "jpeg";
  if (
    bytes[0] === 80 && [50, 51, 53, 54].includes(bytes[1]) &&
    [9, 10, 13, 32].includes(bytes[2])
  ) return "pnm";
  throw new ConnectError(
    "PNG, JPEG vagy PNM képet tölts fel. SVG/DWG most nem támogatott.",
    Code.InvalidArgument,
  );
}

// Bounded session history only; not a durable CAD document/replay database.
export type History = { role: "user" | "assistant"; content: string }[];
type Session = {
  history: History;
  pending?: PendingQuestion;
  imageId?: string;
  touched: number;
  active: boolean;
};
export class SessionStore {
  private entries = new Map<string, Session>();
  acquire(id: string): Session {
    if (!/^[a-zA-Z0-9_-]{8,80}$/.test(id)) {
      throw new ConnectError("Érvénytelen munkamenet.", Code.InvalidArgument);
    }
    for (const [key, value] of this.entries) {
      if (!value.active && Date.now() - value.touched > TTL) {
        this.entries.delete(key);
      }
    }
    let entry = this.entries.get(id);
    if (entry?.active) {
      throw new ConnectError(
        "Ebben a munkamenetben már fut egy művelet.",
        Code.AlreadyExists,
      );
    }
    if (!entry) {
      if (this.entries.size >= 32) {
        throw new ConnectError(
          "Túl sok aktív munkamenet.",
          Code.ResourceExhausted,
        );
      }
      entry = { history: [], touched: Date.now(), active: false };
      this.entries.set(id, entry);
    }
    entry.active = true;
    entry.touched = Date.now();
    return entry;
  }
  release(entry: Session) {
    entry.active = false;
    entry.touched = Date.now();
    entry.history = entry.history.slice(-12);
    // The provider also limits total history characters, not just message count.
    let chars = entry.history.reduce(
      (sum, message) => sum + message.content.length,
      0,
    );
    while (chars > 48_000 && entry.history.length) {
      chars -= entry.history.shift()!.content.length;
    }
  }
}
