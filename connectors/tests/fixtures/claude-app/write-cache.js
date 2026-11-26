// Writes the test cache of the Claude app reader: an IndexedDB of the origin https://claude.ai with
// the database and stores the Claude app keeps, filled with made-up conversations. Electron writes
// it, so the files are in the format of a Chromium web view. No request leaves the machine: every
// https request is answered here.
//
// Run from the repository root, then copy the result over the fixture:
//   app/node_modules/.bin/electron connectors/tests/fixtures/claude-app/write-cache.js <out>
//   <out>/IndexedDB/https_claude.ai_0.indexeddb.{leveldb,blob} -> connectors/tests/fixtures/claude-app/IndexedDB/
// Keep CURRENT, the manifest, the *.log and *.ldb files of the LevelDB directory and the blob
// files; leave out LOCK and LOG. The tests cut the log at lengths found in this fixture
// (see connectors/src/claude_app.rs), so a new fixture needs them found again.

const { app, BrowserWindow, session } = require('electron');

const out = process.argv[process.argv.length - 1];
app.setPath('userData', out);

// The page code that writes the records. It runs in the page, so it is a string.
const write = String.raw`
(async () => {
  // Words from a fixed generator, so a long session cannot be compressed much and the file is
  // the same on every run.
  let seed = 7;
  const words = ['plan', 'река', 'build', 'город', 'test', 'данные', 'merge', 'сессия', 'cache', 'ответ'];
  const text = (count) => {
    const parts = [];
    for (let i = 0; i < count; i += 1) {
      seed = (seed * 1103515245 + 12345) % 2147483648;
      parts.push(words[seed % words.length] + (seed % 1000));
    }
    return parts.join(' ');
  };

  const human = {
    uuid: 'm0000000-0000-4000-8000-000000000001',
    text: '',
    content: [{ type: 'text', text: 'Расскажи мне про Берлин' }],
    sender: 'human',
    index: 0,
    created_at: '2026-11-24T13:47:00.000000Z',
    parent_message_uuid: '00000000-0000-4000-8000-000000000000',
  };
  const assistant = {
    uuid: 'm0000000-0000-4000-8000-000000000002',
    text: '',
    content: [
      { type: 'thinking', thinking: 'The person asks about Berlin.' },
      { type: 'text', text: 'Берлин — столица Германии.' },
    ],
    sender: 'assistant',
    index: 1,
    created_at: '2026-11-24T13:47:05.000000Z',
    parent_message_uuid: human.uuid,
  };
  const chat = {
    conversationUuid: 'c0000000-0000-4000-8000-000000000001',
    product: 'chat',
    fetchedAt: 1795531620000,
    messageCount: 2,
    tree: {
      uuid: 'c0000000-0000-4000-8000-000000000001',
      name: 'Berlin',
      current_leaf_message_uuid: assistant.uuid,
      chat_messages: [human, assistant],
      messageByUuid: { [human.uuid]: human, [assistant.uuid]: assistant },
      parentByChildUuid: { [human.uuid]: human.parent_message_uuid, [assistant.uuid]: human.uuid },
    },
  };

  const event = (seq, role, content) => ({
    kind: 'message',
    dedupKey: 'd' + String(seq).padStart(7, '0'),
    seq,
    payload: {
      session_id: 'cse_fixture',
      type: role,
      uuid: 'u' + String(seq).padStart(7, '0'),
      message: { role, content },
      created_at: '2026-11-24T13:54:' + String(seq % 60).padStart(2, '0') + '.000000Z',
    },
    serverCreatedAt: 1795531640000 + seq,
  });
  const cowork = (key, events, hasOlder) => ({
    conversationUuid: key,
    product: 'cowork',
    messageCount: events.length,
    tree: {
      kind: 'session-events',
      v: 1,
      events,
      headSeq: events[events.length - 1].seq,
      floorSeq: events[0].seq,
      hasOlder,
      olderCursor: undefined,
    },
  });
  const short = [
    event(1, 'user', 'Создай файл cowork-test.txt'),
    event(2, 'assistant', 'Готово: файл создан.'),
  ];
  const long = [];
  for (let seq = 7; seq < 127; seq += 1) {
    long.push(event(seq, seq % 2 ? 'user' : 'assistant', text(160)));
  }
  // A session that compresses well: kept in the database, compressed.
  const medium = [];
  for (let seq = 1; seq < 41; seq += 1) {
    medium.push(event(seq, seq % 2 ? 'user' : 'assistant', 'повтор '.repeat(120)));
  }

  const db = await new Promise((resolve, reject) => {
    const open = indexedDB.open('claude-conversation-store', 1);
    open.onupgradeneeded = () => {
      open.result.createObjectStore('trees');
      open.result.createObjectStore('meta');
    };
    open.onsuccess = () => resolve(open.result);
    open.onerror = () => reject(open.error);
  });
  const step = (stores, run) =>
    new Promise((resolve, reject) => {
      const tx = db.transaction(stores, 'readwrite');
      run(tx);
      tx.oncomplete = resolve;
      tx.onerror = () => reject(tx.error);
    });

  await step(['trees', 'meta'], (tx) => {
    tx.objectStore('trees').put(chat, chat.conversationUuid);
    tx.objectStore('trees').put(cowork('cowork:cse_fixture_short', short.slice(0, 1), false), 'cowork:cse_fixture_short');
    tx.objectStore('trees').put(cowork('cowork:cse_fixture_gone', short, false), 'cowork:cse_fixture_gone');
    tx.objectStore('meta').put({ product: 'chat', lastOpenedAt: 1795531620000 }, chat.conversationUuid);
  });
  // A newer version of one record, and a record that is deleted.
  await step(['trees'], (tx) => {
    tx.objectStore('trees').put(cowork('cowork:cse_fixture_short', short, false), 'cowork:cse_fixture_short');
    tx.objectStore('trees').delete('cowork:cse_fixture_gone');
  });
  // A session large enough to be kept in a blob file, and one kept compressed in the database.
  await step(['trees'], (tx) => {
    tx.objectStore('trees').put(cowork('cowork:cse_fixture_long', long, true), 'cowork:cse_fixture_long');
    tx.objectStore('trees').put(cowork('cowork:cse_fixture_medium', medium, false), 'cowork:cse_fixture_medium');
  });
  db.close();
  return 'written';
})()
`;

app.whenReady().then(async () => {
  session.defaultSession.protocol.handle(
    'https',
    () => new Response('<!doctype html><title>cache</title>', { headers: { 'content-type': 'text/html' } }),
  );
  const window = new BrowserWindow({ show: false });
  await window.loadURL('https://claude.ai/');
  console.log(await window.webContents.executeJavaScript(write));
  window.destroy();
  app.quit();
});
