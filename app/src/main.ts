import path from 'node:path';
import { app, BrowserWindow, ipcMain } from 'electron';
import started from 'electron-squirrel-startup';
import { agentBinaryPath, ensureAgentService, shouldRegisterAgent } from './agent-service';
import { readAgentStatus } from './agent-socket';
import { STATUS_CHANNEL } from './agent-status';

let mainWindow: BrowserWindow | null = null;

// On first start the agent registers itself as a login service. A failure leaves the window
// on `stopped`, so it is logged and the app goes on.
const registerAgent = async () => {
  if (!shouldRegisterAgent(app.isPackaged, process.env)) {
    console.info('agent registration skipped: development run');
    return;
  }
  const binary = agentBinaryPath({
    packaged: app.isPackaged,
    resourcesPath: process.resourcesPath,
    appPath: app.getAppPath(),
  });
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
    if (mainWindow === null) {
      createWindow();
      return;
    }
    if (mainWindow.isMinimized()) {
      mainWindow.restore();
    }
    mainWindow.focus();
  });

  void app.whenReady().then(() => {
    ipcMain.handle(STATUS_CHANNEL, () => readAgentStatus(process.env, process.platform));
    void registerAgent();
    createWindow();

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
