import { existsSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { app, BrowserWindow, dialog, ipcMain, shell } from 'electron';
import started from 'electron-squirrel-startup';
import {
  agentBinaryPath,
  ensureAgentService,
  isInstalledDiskPath,
  registrationDecision,
} from './agent-service';
import {
  SETTINGS_COLLECTION_CHANNEL,
  SETTINGS_GET_CHANNEL,
  SETTINGS_REMOVE_KEY_CHANNEL,
  SETTINGS_SAVE_CHANNEL,
  SETTINGS_TEST_CHANNEL,
  SETTINGS_TEST_PROGRESS_CHANNEL,
} from './agent-settings';
import { readAgentStatus } from './agent-socket';
import { NEEDS_MOVE_CHANNEL, STATUS_CHANNEL } from './agent-status';
import { createChatHandlers } from './chat-calls';
import type { ChatHandlers } from './chat-calls';
import {
  CHAT_DELETE_CHANNEL,
  CHAT_EVENT_CHANNEL,
  CHAT_GET_CHANNEL,
  CHAT_LIST_CHANNEL,
  CHAT_SEND_CHANNEL,
  CHAT_STOP_CHANNEL,
} from './chat-protocol';
import { createDashboardHandlers } from './dashboard-calls';
import { DASHBOARD_NUMBERS_CHANNEL } from './dashboard-protocol';
import { createLinkHandler } from './external-calls';
import { OPEN_LINK_CHANNEL } from './external';
import { offerMoveToApplications } from './move-to-applications';
import { createSettingsHandlers } from './settings-calls';

let mainWindow: BrowserWindow | null = null;
let chatHandlers: ChatHandlers | null = null;

let needsMove = false;
// A second launch has nothing to open until the first one has finished starting.
let startupDone = false;

const moveDeclinedMarker = () => path.join(app.getPath('userData'), 'move-declined');

// A packaged app outside Applications asks whether to move itself there. Electron's own call
// shows no dialog, so the question is ours. Returns `true` when the app is moving and this run
// must stop.
const moveToApplications = (): Promise<boolean> => {
  return offerMoveToApplications({
    isEligible: () =>
      process.platform === 'darwin' &&
      app.isPackaged &&
      !app.isInApplicationsFolder() &&
      !isInstalledDiskPath(process.resourcesPath),
    isDeclined: () => existsSync(moveDeclinedMarker()),
    markDeclined: () => {
      writeFileSync(moveDeclinedMarker(), '');
    },
    ask: async () => {
      const { response } = await dialog.showMessageBox({
        type: 'question',
        message: 'Move Construct Browser to the Applications folder?',
        detail: 'The agent starts at login only when the app is installed in Applications.',
        buttons: ['Move to Applications', 'Not now'],
        defaultId: 0,
        cancelId: 1,
      });
      return response === 0;
    },
    move: (conflictHandler) => app.moveToApplicationsFolder({ conflictHandler }),
  });
};

// On first start the agent registers itself as a login service. A failure leaves the window
// on `stopped`, so it is logged and the app goes on. From a place macOS removes later, nothing
// is registered and the window tells the user to move the app.
const registerAgent = async () => {
  const binary = agentBinaryPath({
    packaged: app.isPackaged,
    resourcesPath: process.resourcesPath,
    appPath: app.getAppPath(),
  });
  const decision = registrationDecision(app.isPackaged, process.env, binary, process.platform);
  if (decision === 'skip-development') {
    console.info('agent registration skipped: development run');
    return;
  }
  if (decision === 'skip-transient-path') {
    needsMove = true;
    console.warn(
      `agent registration skipped: the app runs from ${binary}; move it to Applications`,
    );
    return;
  }
  try {
    console.info(await ensureAgentService(binary, process.env));
  } catch (error) {
    console.error(error);
  }
};

const createWindow = () => {
  const window = new BrowserWindow({
    width: 960,
    height: 640,
    minWidth: 720,
    minHeight: 480,
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  window.webContents.on('will-navigate', (event) => {
    event.preventDefault();
  });
  window.on('closed', () => {
    mainWindow = null;
    // The window is gone, and so is the one who wanted the answer.
    chatHandlers?.close();
  });

  if (MAIN_WINDOW_VITE_DEV_SERVER_URL) {
    void window.loadURL(MAIN_WINDOW_VITE_DEV_SERVER_URL);
  } else {
    void window.loadFile(path.join(__dirname, `../renderer/${MAIN_WINDOW_VITE_NAME}/index.html`));
  }
  mainWindow = window;
};

// A second launch focuses the window of the first one (or opens one, on macOS, where the app
// can stay alive without a window) and quits.
function startApp(): void {
  app.on('second-instance', () => {
    if (!startupDone) {
      return;
    }
    if (mainWindow === null) {
      createWindow();
      return;
    }
    if (mainWindow.isMinimized()) {
      mainWindow.restore();
    }
    mainWindow.focus();
  });

  void app.whenReady().then(async () => {
    ipcMain.handle(NEEDS_MOVE_CHANNEL, () => needsMove);
    ipcMain.handle(STATUS_CHANNEL, () => readAgentStatus(process.env, process.platform));
    const settings = createSettingsHandlers(
      { env: process.env, platform: process.platform },
      (progress) => {
        if (mainWindow !== null && !mainWindow.isDestroyed()) {
          mainWindow.webContents.send(SETTINGS_TEST_PROGRESS_CHANNEL, progress);
        }
      },
    );
    ipcMain.handle(SETTINGS_GET_CHANNEL, () => settings.get());
    ipcMain.handle(SETTINGS_SAVE_CHANNEL, (_event, input: unknown) => settings.save(input));
    ipcMain.handle(SETTINGS_TEST_CHANNEL, (_event, input: unknown) => settings.test(input));
    ipcMain.handle(SETTINGS_REMOVE_KEY_CHANNEL, () => settings.removeKey());
    // The answer's events go to the window that is open; with no window there is no one to tell.
    const chat = createChatHandlers({ env: process.env, platform: process.platform }, (event) => {
      if (mainWindow !== null && !mainWindow.isDestroyed()) {
        mainWindow.webContents.send(CHAT_EVENT_CHANNEL, event);
      }
    });
    chatHandlers = chat;
    ipcMain.handle(CHAT_LIST_CHANNEL, () => chat.list());
    ipcMain.handle(CHAT_GET_CHANNEL, (_event, conversation: unknown) => chat.get(conversation));
    ipcMain.handle(CHAT_DELETE_CHANNEL, (_event, which: unknown) => chat.remove(which));
    ipcMain.handle(CHAT_SEND_CHANNEL, (_event, input: unknown) => chat.send(input));
    ipcMain.handle(CHAT_STOP_CHANNEL, () => chat.stop());
    app.on('before-quit', chat.close);
    const dashboard = createDashboardHandlers({ env: process.env, platform: process.platform });
    ipcMain.handle(DASHBOARD_NUMBERS_CHANNEL, (_event, input: unknown) => dashboard.numbers(input));
    // The window never opens a page itself: a link of an answer comes here, and only a web address
    // goes on to the system browser.
    const openLink = createLinkHandler((address) => shell.openExternal(address));
    ipcMain.handle(OPEN_LINK_CHANNEL, (_event, url: unknown) => openLink(url));
    ipcMain.handle(SETTINGS_COLLECTION_CHANNEL, (_event, input: unknown) =>
      settings.saveCollection(input),
    );
    // The handlers come first: a second launch can open a window while the dialog is up.
    if (await moveToApplications()) {
      return;
    }
    void registerAgent();
    createWindow();
    startupDone = true;

    app.on('activate', () => {
      if (BrowserWindow.getAllWindows().length === 0) {
        createWindow();
      }
    });
  });
}

// Squirrel runs the app only to create or remove shortcuts on Windows; it must not start a window.
if (started || !app.requestSingleInstanceLock()) {
  app.quit();
} else {
  startApp();
}

// The agent is its own process and keeps running when the app quits.
app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});
