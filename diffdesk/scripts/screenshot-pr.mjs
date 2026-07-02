#!/usr/bin/env node
import { spawn, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import process from "node:process";

const DEFAULT_BASE_REF = "origin/main";
const DEFAULT_HEAD_REF = "HEAD";
const DEFAULT_VIEWPORT = { width: 1440, height: 920 };
const APP_SUBDIR = "diffdesk";

const RAW_DIFF = String.raw`diff --git a/diffdesk/src/App.tsx b/diffdesk/src/App.tsx
index aeb3c59..c17256b 100644
--- a/diffdesk/src/App.tsx
+++ b/diffdesk/src/App.tsx
@@ -562,10 +562,7 @@ function TitleBar({
 }) {
   return (
     <div className="titlebar">
-      <div className="titlebar__left">
-        <TrafficLights />
-      </div>
-      <div className="titlebar__center">
+      <div className="titlebar__meta">
         <div className="titlebar__branch">
           <GitBranch size={12} />
           <span className="branch-name">{sourceHead(session)}</span>
@@ -613,16 +610,6 @@ function TitleBar({
   );
 }
 
-function TrafficLights() {
-  return (
-    <div className="traffic-lights" aria-hidden="true">
-      <span className="traffic-dot close" />
-      <span className="traffic-dot minimize" />
-      <span className="traffic-dot zoom" />
-    </div>
-  );
-}
-
 function FindBar({
   inputRef,
   matchCount,
diff --git a/diffdesk/src/styles/app.css b/diffdesk/src/styles/app.css
index 600cd31..59d35e9 100644
--- a/diffdesk/src/styles/app.css
+++ b/diffdesk/src/styles/app.css
@@ -96,11 +96,7 @@ button {
 .desktop-bg {
   width: 100vw;
   height: 100vh;
-  padding: 14px;
-  background:
-    radial-gradient(circle at 20% 0%, #f4ece8 0%, transparent 50%),
-    radial-gradient(circle at 90% 100%, #efe6e0 0%, transparent 55%),
-    var(--warm-50);
+  background: var(--surface-page);
   color: var(--content-body);
 }
 
@@ -115,12 +111,6 @@ button {
   display: flex;
   flex-direction: column;
   background: var(--surface-background);
-  border-radius: 12px;
-  box-shadow:
-    0 0 0 0.5px rgba(0, 0, 0, 0.18),
-    0 1px 0 rgba(255, 255, 255, 0.6) inset,
-    0 24px 64px -16px rgba(40, 37, 36, 0.28),
-    0 8px 24px -8px rgba(40, 37, 36, 0.16);
 }
 
 .loading-window {
@@ -141,7 +131,8 @@ button {
   height: 48px;
   flex-shrink: 0;
   display: grid;
-  grid-template-columns: 220px 1fr auto;
+  grid-template-columns: minmax(0, 1fr) auto;
+  column-gap: 16px;
   align-items: center;
   padding: 0 14px 0 18px;
   background: linear-gradient(180deg, #f5efeb 0%, #efe8e3 100%);
@@ -149,15 +140,15 @@ button {
   -webkit-app-region: drag;
 }
 
-.titlebar__left,
-.titlebar__center,
+.titlebar__meta,
 .titlebar__right {
   display: flex;
   align-items: center;
+  min-width: 0;
 }
 
-.titlebar__center {
-  justify-content: center;
+.titlebar__meta {
+  justify-content: flex-start;
 }`;

const SESSION = {
  schemaVersion: "1",
  sessionId: "ses_screenshot_pr_001",
  createdAt: "2026-07-02T12:00:00.000Z",
  sessionDir: "/tmp/diffdesk/session",
  inputDiffPath: "/tmp/diffdesk/input.diff",
  source: {
    kind: "git",
    repoRoot: "/tmp/atelier",
    workingDirectory: "/tmp/atelier",
    range: "main...feature/window-chrome",
    staged: false,
    all: false,
  },
  options: {
    wait: false,
    outputPath: null,
    outputFormat: "markdown",
    copyToClipboard: false,
    aiCommand: null,
  },
};

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    printHelp();
    return;
  }

  const repoRoot = gitOutput(["rev-parse", "--show-toplevel"], process.cwd());
  const prNumber = options.pr ?? detectPrNumber(repoRoot) ?? "local";
  const outDir = path.resolve(
    repoRoot,
    APP_SUBDIR,
    options.out ?? path.join("docs", "pr-assets", `pr-${prNumber}`),
  );
  const chromePath = findChrome();
  const tempRoot = await mkdtemp(path.join(os.tmpdir(), "diffdesk-shots-"));
  const worktrees = [];

  await mkdir(outDir, { recursive: true });

  try {
    const targets = [
      { label: "before", ref: options.base },
      { label: "after", ref: options.head },
    ];
    const results = [];

    for (const target of targets) {
      const worktree = await addWorktree(
        repoRoot,
        tempRoot,
        target.label,
        target.ref,
      );
      worktrees.push(worktree);
      const appDir = path.join(worktree, APP_SUBDIR);
      const outFile = path.join(outDir, `${target.label}.png`);
      const metrics = await renderTarget({
        appDir,
        chromePath,
        install: options.install,
        label: target.label,
        outFile,
        viewport: options.viewport,
      });
      results.push({ ...target, file: outFile, metrics });
    }

    console.log(JSON.stringify({ outDir, results }, null, 2));
  } finally {
    for (const worktree of worktrees.reverse()) {
      await removeWorktree(repoRoot, worktree);
    }
    if (!options.keep) {
      await rm(tempRoot, { recursive: true, force: true });
    } else {
      console.log(`Kept temp directory: ${tempRoot}`);
    }
  }
}

function parseArgs(args) {
  const options = {
    base: DEFAULT_BASE_REF,
    head: DEFAULT_HEAD_REF,
    help: false,
    install: true,
    keep: false,
    out: null,
    pr: null,
    viewport: DEFAULT_VIEWPORT,
  };

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    switch (arg) {
      case "--base":
        options.base = readValue(args, (index += 1), arg);
        break;
      case "--head":
        options.head = readValue(args, (index += 1), arg);
        break;
      case "--out":
        options.out = readValue(args, (index += 1), arg);
        break;
      case "--pr":
        options.pr = readValue(args, (index += 1), arg);
        break;
      case "--viewport":
        options.viewport = parseViewport(readValue(args, (index += 1), arg));
        break;
      case "--skip-install":
        options.install = false;
        break;
      case "--keep":
        options.keep = true;
        break;
      case "--help":
      case "-h":
        options.help = true;
        break;
      default:
        throw new Error(`Unknown argument: ${arg}`);
    }
  }

  return options;
}

function readValue(args, index, flag) {
  const value = args[index];
  if (!value || value.startsWith("--")) {
    throw new Error(`${flag} requires a value`);
  }
  return value;
}

function parseViewport(value) {
  const match = value.match(/^(\d+)x(\d+)$/);
  if (!match) {
    throw new Error("--viewport must use WIDTHxHEIGHT, for example 1440x920");
  }
  return {
    width: Number.parseInt(match[1], 10),
    height: Number.parseInt(match[2], 10),
  };
}

function printHelp() {
  console.log(`Capture Diffdesk before/after PR screenshots.

Usage:
  pnpm screenshot:pr [--base origin/main] [--head HEAD] [--pr 53]

Options:
  --base REF        Ref used for before.png. Defaults to origin/main.
  --head REF        Ref used for after.png. Defaults to HEAD.
  --out PATH        Output directory, relative to diffdesk/. Defaults to docs/pr-assets/pr-<number>.
  --pr NUMBER       PR number for the default output path.
  --viewport WxH    Screenshot viewport. Defaults to 1440x920.
  --skip-install    Skip pnpm install in temporary worktrees.
  --keep            Keep temporary worktrees after the run.
`);
}

async function addWorktree(repoRoot, tempRoot, label, ref) {
  const worktree = path.join(tempRoot, label);
  console.log(`Creating ${label} worktree from ${ref}`);
  await run("git", ["worktree", "add", "--detach", worktree, ref], {
    cwd: repoRoot,
  });
  return worktree;
}

async function removeWorktree(repoRoot, worktree) {
  try {
    await run("git", ["worktree", "remove", "--force", worktree], {
      cwd: repoRoot,
    });
  } catch (error) {
    console.warn(`Could not remove worktree via git: ${error.message}`);
    await rm(worktree, { recursive: true, force: true });
  }
}

async function renderTarget({
  appDir,
  chromePath,
  install,
  label,
  outFile,
  viewport,
}) {
  if (install) {
    console.log(`Installing dependencies for ${label}`);
    await run("pnpm", ["install", "--frozen-lockfile"], { cwd: appDir });
  }

  console.log(`Starting Vite for ${label}`);
  const server = await startVite(appDir);
  try {
    console.log(`Capturing ${label} to ${outFile}`);
    return await captureScreenshot({
      appUrl: server.url,
      chromePath,
      label,
      outFile,
      viewport,
    });
  } finally {
    await server.stop();
  }
}

async function startVite(appDir) {
  const port = await freePort();
  const url = `http://127.0.0.1:${port}/`;
  const child = spawn(
    "pnpm",
    ["exec", "vite", "--host", "127.0.0.1", "--port", String(port)],
    {
      cwd: appDir,
      env: { ...process.env, CI: "1" },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  let output = "";
  child.stdout.on("data", (chunk) => {
    output += chunk;
  });
  child.stderr.on("data", (chunk) => {
    output += chunk;
  });

  await Promise.race([
    waitForHttp(url, 15000),
    new Promise((_, reject) => {
      child.once("exit", (code) => {
        reject(new Error(`Vite exited with ${code}\n${output}`));
      });
    }),
  ]);

  return {
    url,
    stop: () => stopChild(child),
  };
}

async function captureScreenshot({
  appUrl,
  chromePath,
  label,
  outFile,
  viewport,
}) {
  const remotePort = await freePort();
  const userDataDir = await mkdtemp(path.join(os.tmpdir(), "diffdesk-chrome-"));
  const chrome = spawn(
    chromePath,
    [
      "--headless=new",
      `--remote-debugging-port=${remotePort}`,
      `--user-data-dir=${userDataDir}`,
      `--window-size=${viewport.width},${viewport.height}`,
      "--force-device-scale-factor=1",
      "--disable-gpu",
      "--no-first-run",
      "--no-default-browser-check",
      "about:blank",
    ],
    { stdio: "ignore" },
  );

  let client = null;
  try {
    await waitForJson(`http://127.0.0.1:${remotePort}/json/version`, 10000);
    const pages = await waitForJson(
      `http://127.0.0.1:${remotePort}/json/list`,
      10000,
    );
    const page = pages.find((entry) => entry.type === "page");
    if (!page?.webSocketDebuggerUrl) {
      throw new Error("No debuggable Chrome page found");
    }

    client = await connectCdp(page.webSocketDebuggerUrl);
    await client.send("Page.enable");
    await client.send("Runtime.enable");
    await client.send("Emulation.setDeviceMetricsOverride", {
      width: viewport.width,
      height: viewport.height,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await client.send("Page.addScriptToEvaluateOnNewDocument", {
      source: tauriMockSource(),
    });
    await client.send("Page.navigate", { url: appUrl });
    await waitForExpression(
      client,
      "Boolean(document.querySelector('.titlebar') && document.querySelector('.file'))",
      15000,
    );
    await delay(250);

    const metrics = await evaluateMetrics(client, label);
    const screenshot = await client.send("Page.captureScreenshot", {
      format: "png",
      fromSurface: true,
    });
    await writeFile(outFile, Buffer.from(screenshot.data, "base64"));
    return metrics;
  } finally {
    client?.close();
    await stopChild(chrome);
    await rm(userDataDir, { recursive: true, force: true });
  }
}

async function evaluateMetrics(client, label) {
  const result = await client.send("Runtime.evaluate", {
    expression: `(() => {
      const desktop = document.querySelector(".desktop-bg");
      const win = document.querySelector(".window");
      const titlebar = document.querySelector(".titlebar");
      const desktopStyle = getComputedStyle(desktop);
      const winStyle = getComputedStyle(win);
      return {
        label: ${JSON.stringify(label)},
        trafficCount: document.querySelectorAll(".traffic-lights,.traffic-dot").length,
        titlebarChildren: Array.from(titlebar.children).map((node) => node.className),
        desktopPadding: desktopStyle.padding,
        windowBorderRadius: winStyle.borderRadius,
        windowBoxShadow: winStyle.boxShadow
      };
    })()`,
    returnByValue: true,
  });
  return result.result.value;
}

function tauriMockSource() {
  return `(() => {
    const session = ${JSON.stringify(SESSION)};
    const rawDiff = ${JSON.stringify(RAW_DIFF)};
    window.__TAURI_INTERNALS__ = {
      transformCallback: () => Math.floor(Math.random() * 1000000),
      unregisterCallback: () => {},
      invoke: async (cmd) => {
        if (cmd === "current_session_id") return session.sessionId;
        if (cmd === "load_session") return { session, rawDiff, drafts: null };
        if (cmd === "save_drafts") return null;
        if (cmd === "submit_review") {
          return {
            schemaVersion: "1",
            sessionId: session.sessionId,
            status: "ok",
            submittedAt: new Date().toISOString(),
            outputPath: null,
            resultPath: "/tmp/diffdesk/result.json"
          };
        }
        throw new Error("Unhandled mock invoke: " + cmd);
      }
    };
  })();`;
}

function connectCdp(wsUrl) {
  if (typeof WebSocket !== "function") {
    throw new Error("This script needs Node.js with a global WebSocket API");
  }

  return new Promise((resolve, reject) => {
    const ws = new WebSocket(wsUrl);
    const pending = new Map();
    const events = [];
    let id = 0;

    ws.onopen = () => {
      resolve({
        close() {
          ws.close();
        },
        events,
        send(method, params = {}) {
          const messageId = ++id;
          ws.send(JSON.stringify({ id: messageId, method, params }));
          return new Promise((resolveCommand, rejectCommand) => {
            pending.set(messageId, {
              reject: rejectCommand,
              resolve: resolveCommand,
            });
          });
        },
      });
    };
    ws.onerror = reject;
    ws.onmessage = (event) => {
      const message = JSON.parse(event.data);
      if (message.id && pending.has(message.id)) {
        const command = pending.get(message.id);
        pending.delete(message.id);
        if (message.error) {
          command.reject(new Error(JSON.stringify(message.error)));
        } else {
          command.resolve(message.result);
        }
      } else {
        events.push(message);
      }
    };
  });
}

async function waitForExpression(client, expression, timeoutMs) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    const result = await client.send("Runtime.evaluate", {
      expression,
      returnByValue: true,
      awaitPromise: true,
    });
    if (result.result?.value) return result.result.value;
    await delay(100);
  }
  throw new Error(`Timed out waiting for expression: ${expression}`);
}

function waitForHttp(url, timeoutMs) {
  const startedAt = Date.now();
  return new Promise((resolve, reject) => {
    const attempt = () => {
      http
        .get(url, (response) => {
          response.resume();
          if (response.statusCode && response.statusCode < 500) {
            resolve();
          } else if (Date.now() - startedAt >= timeoutMs) {
            reject(new Error(`Timed out waiting for ${url}`));
          } else {
            setTimeout(attempt, 100);
          }
        })
        .on("error", () => {
          if (Date.now() - startedAt >= timeoutMs) {
            reject(new Error(`Timed out waiting for ${url}`));
          } else {
            setTimeout(attempt, 100);
          }
        });
    };
    attempt();
  });
}

function waitForJson(url, timeoutMs) {
  const startedAt = Date.now();
  return new Promise((resolve, reject) => {
    const attempt = () => {
      http
        .get(url, (response) => {
          let body = "";
          response.setEncoding("utf8");
          response.on("data", (chunk) => {
            body += chunk;
          });
          response.on("end", () => {
            try {
              resolve(JSON.parse(body));
            } catch {
              if (Date.now() - startedAt >= timeoutMs) {
                reject(new Error(`Timed out waiting for JSON from ${url}`));
              } else {
                setTimeout(attempt, 100);
              }
            }
          });
        })
        .on("error", () => {
          if (Date.now() - startedAt >= timeoutMs) {
            reject(new Error(`Timed out waiting for JSON from ${url}`));
          } else {
            setTimeout(attempt, 100);
          }
        });
    };
    attempt();
  });
}

function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = address.port;
      server.close(() => resolve(port));
    });
    server.on("error", reject);
  });
}

function delay(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function run(command, args, { cwd, env } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env: env ?? process.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolve({ stdout, stderr });
      } else {
        reject(
          new Error(
            [
              `Command failed (${code}): ${command} ${args.join(" ")}`,
              stdout,
              stderr,
            ]
              .filter(Boolean)
              .join("\n"),
          ),
        );
      }
    });
  });
}

function stopChild(child) {
  if (child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve();
  }

  return new Promise((resolve) => {
    const timeout = setTimeout(() => {
      child.kill("SIGKILL");
    }, 2000);
    child.once("exit", () => {
      clearTimeout(timeout);
      resolve();
    });
    child.kill("SIGTERM");
  });
}

function gitOutput(args, cwd) {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf8",
  });
  if (result.status !== 0) {
    throw new Error(result.stderr || `git ${args.join(" ")} failed`);
  }
  return result.stdout.trim();
}

function detectPrNumber(repoRoot) {
  const result = spawnSync(
    "gh",
    ["pr", "view", "--json", "number", "--jq", ".number"],
    {
      cwd: repoRoot,
      encoding: "utf8",
    },
  );
  if (result.status !== 0) return null;
  return result.stdout.trim() || null;
}

function findChrome() {
  const candidates = [
    process.env.CHROME_BIN,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    which("google-chrome"),
    which("chromium"),
    which("chrome"),
  ].filter(Boolean);

  const chromePath = candidates.find((candidate) => existsSync(candidate));
  if (!chromePath) {
    throw new Error("Could not find Chrome. Set CHROME_BIN to a Chrome binary.");
  }
  return chromePath;
}

function which(command) {
  const result = spawnSync("which", [command], { encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : null;
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
