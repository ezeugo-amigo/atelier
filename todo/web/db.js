// db.js — IndexedDB persistence wired to Elm ports.
//
// The whole task list is stored as a single value under one key. That keeps the
// Elm side simple (it owns the list; JS is a dumb key/value sink) and a write is
// one transaction regardless of how many tasks changed.

const DB_NAME = "today-todo";
const DB_VERSION = 1;
const STORE = "kv";
const KEY = "tasks";

function openDb() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(STORE)) {
        db.createObjectStore(STORE);
      }
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function loadTasks() {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const req = db.transaction(STORE, "readonly").objectStore(STORE).get(KEY);
    req.onsuccess = () => resolve(req.result); // undefined when nothing stored yet
    req.onerror = () => reject(req.error);
  });
}

async function saveTasks(tasks) {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).put(tasks, KEY);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

// Connect the running Elm program's ports to IndexedDB.
export function wire(app) {
  app.ports.dbLoad.subscribe(async () => {
    let tasks;
    try {
      tasks = await loadTasks();
    } catch (e) {
      console.warn("today: IndexedDB load failed, starting fresh", e);
      tasks = undefined;
    }
    const found = Array.isArray(tasks);
    app.ports.dbLoaded.send({ found, tasks: found ? tasks : [] });
  });

  app.ports.dbSave.subscribe(async (tasks) => {
    try {
      await saveTasks(tasks);
    } catch (e) {
      console.warn("today: IndexedDB save failed", e);
    }
  });
}
