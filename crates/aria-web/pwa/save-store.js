export class IndexedDbSaveStore {
  constructor(databaseName, keepGenerations = 3) {
    this.databaseName = databaseName;
    this.keepGenerations = Math.max(2, keepGenerations);
    this.database = null;
  }

  open() {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open(this.databaseName, 1);
      request.onupgradeneeded = () => {
        const store = request.result.createObjectStore("generations", { keyPath: "id" });
        store.createIndex("slot", "slotKey", { unique: false });
      };
      request.onsuccess = () => {
        this.database = request.result;
        resolve();
      };
      request.onerror = () => reject(request.error);
    });
  }

  async put(namespace, slot, payload) {
    const slotKey = `${namespace}:${slot}`;
    const transaction = this.database.transaction("generations", "readwrite");
    const store = transaction.objectStore("generations");
    const records = await requestResult(store.index("slot").getAll(slotKey));
    const generation = records.reduce((max, record) => Math.max(max, record.generation), 0) + 1;
    store.put({
      id: `${slotKey}:${generation}`,
      slotKey,
      generation,
      payload,
      writtenAt: Date.now(),
    });
    records
      .sort((left, right) => right.generation - left.generation)
      .slice(this.keepGenerations - 1)
      .forEach((record) => store.delete(record.id));
    await transactionComplete(transaction);
    return generation;
  }

  async generations(namespace, slot) {
    const slotKey = `${namespace}:${slot}`;
    const transaction = this.database.transaction("generations", "readonly");
    const records = await requestResult(
      transaction.objectStore("generations").index("slot").getAll(slotKey),
    );
    await transactionComplete(transaction);
    return records.sort((left, right) => right.generation - left.generation);
  }
}
function requestResult(request) {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

function transactionComplete(transaction) {
  return new Promise((resolve, reject) => {
    transaction.oncomplete = resolve;
    transaction.onabort = () => reject(transaction.error);
    transaction.onerror = () => reject(transaction.error);
  });
}
