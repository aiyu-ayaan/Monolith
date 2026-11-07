export interface SyncStatus {
  online: boolean;
  pendingCount: number;
  lastSyncedAt?: string;
}

export type SyncStatusListener = (status: SyncStatus) => void;

export class SyncEngine {
  private collections: Set<string> = new Set();
  private listeners: Set<SyncStatusListener> = new Set();
  private isOnline = true;

  async enable(collections: string[]): Promise<void> {
    for (const c of collections) {
      this.collections.add(c);
    }
  }

  onStatus(listener: SyncStatusListener): () => void {
    this.listeners.add(listener);
    listener({
      online: this.isOnline,
      pendingCount: 0,
    });
    return () => {
      this.listeners.delete(listener);
    };
  }
}

export const sync = new SyncEngine();
export default sync;

