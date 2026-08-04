#!/usr/bin/env node
// Drives the find bar in a headless Chrome against a fixture diff and asserts
// that highlight ranges land on the right text. Run: node scripts/verify-find.mjs
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import process from "node:process";

const VIEWPORT = { width: 1440, height: 1000 };
const APP_DIR = path.resolve(import.meta.dirname, "..");

const RAW_DIFF = String.raw`diff --git a/src/alpha.ts b/src/alpha.ts
index 1111111..2222222 100644
--- a/src/alpha.ts
+++ b/src/alpha.ts
@@ -1,4 +1,5 @@
 const needle = 1;
-const other = needle + needle;
+const other = needle + needle + needle;
+const extra = "needle";
 export { other };
@@ -40,3 +41,4 @@ function tail() {
   return needle;
 }
+// trailing needle
diff --git a/src/needle-named.ts b/src/needle-named.ts
index 3333333..4444444 100644
--- a/src/needle-named.ts
+++ b/src/needle-named.ts
@@ -1,2 +1,3 @@
 export const value = 1;
+export const another = 2;
 export default value;
`;

const SESSION = {
  schemaVersion: "1",
  sessionId: "ses_verify_find_001",
  createdAt: "2026-08-04T12:00:00.000Z",
  sessionDir: "/tmp/diffdesk/session",
  inputDiffPath: "/tmp/diffdesk/input.diff",
  source: {
    kind: "git",
    repoRoot: "/tmp/atelier",
    workingDirectory: "/tmp/atelier",
    range: "main...find-verify",
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
  const chromePath = findChrome();
  const server = await startVite(APP_DIR);
  const failures = [];
  try {
    const report = await drive(chromePath, server.url);
    console.log(JSON.stringify(report, null, 2));
    for (const [name, check] of Object.entries(report.checks)) {
      if (!check.pass) failures.push(`${name}: ${check.detail}`);
    }
  } finally {
    await server.stop();
  }
  if (failures.length > 0) {
    console.error("\nFAIL\n" + failures.join("\n"));
    process.exitCode = 1;
  } else {
    console.error("\nPASS");
  }
}

async function drive(chromePath, appUrl) {
  const remotePort = await freePort();
  const userDataDir = await mkdtemp(path.join(os.tmpdir(), "diffdesk-find-"));
  const chrome = spawn(
    chromePath,
    [
      "--headless=new",
      `--remote-debugging-port=${remotePort}`,
      `--user-data-dir=${userDataDir}`,
      `--window-size=${VIEWPORT.width},${VIEWPORT.height}`,
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
    await waitForJson(`http://127.0.0.1:${remotePort}/json/version`, 15000);
    const pages = await waitForJson(
      `http://127.0.0.1:${remotePort}/json/list`,
      15000,
    );
    const page = pages.find((entry) => entry.type === "page");
    if (!page?.webSocketDebuggerUrl) throw new Error("no debuggable page");

    client = await connectCdp(page.webSocketDebuggerUrl);
    await client.send("Page.enable");
    await client.send("Runtime.enable");
    await client.send("Emulation.setDeviceMetricsOverride", {
      width: VIEWPORT.width,
      height: VIEWPORT.height,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await client.send("Page.addScriptToEvaluateOnNewDocument", {
      source: tauriMockSource(),
    });
    await client.send("Page.navigate", { url: appUrl });
    await waitForExpression(
      client,
      "Boolean(document.querySelector('.file'))",
      20000,
    );
    await delay(600);

    const checks = {};

    // 1. Cmd+F opens the bar.
    await evaluate(client, `window.__find.pressCmdF(document.body)`);
    await delay(200);
    checks.opensOnCmdF = expect(
      await evaluate(client, `Boolean(document.querySelector('.findbar'))`),
      true,
      "find bar present after Cmd+F",
    );

    // 2. Typing a query highlights every occurrence, not one per line.
    await evaluate(client, `window.__find.type("needle")`);
    await delay(900);
    const counter = await evaluate(
      client,
      `document.querySelector('.findbar__count').textContent`,
    );
    const ranges = await evaluate(client, `window.__find.highlightTexts()`);
    checks.highlightsAllOccurrences = expect(
      ranges.total >= 9,
      true,
      `expected >=9 painted ranges, got ${ranges.total} (counter "${counter}")`,
    );
    checks.everyRangeIsTheNeedle = expect(
      ranges.texts.every((text) => text.toLowerCase() === "needle"),
      true,
      `painted range texts: ${JSON.stringify(ranges.texts.slice(0, 20))}`,
    );
    checks.counterMatchesModel = expect(
      counter,
      `1 of ${ranges.matchCount}`,
      `counter "${counter}" vs model match count ${ranges.matchCount}`,
    );

    // 3. A file-path match paints inside the sidebar-side path element.
    checks.pathMatchHighlighted = expect(
      ranges.hosts.includes("file__path"),
      true,
      `highlight hosts: ${JSON.stringify(ranges.hosts)}`,
    );

    // 4. Active highlight advances with Enter and stays on the needle.
    const firstActive = await evaluate(client, `window.__find.activeInfo()`);
    await evaluate(client, `window.__find.pressEnter()`);
    await delay(700);
    const secondActive = await evaluate(client, `window.__find.activeInfo()`);
    checks.enterAdvancesActive = expect(
      firstActive.key !== secondActive.key,
      true,
      `active before ${JSON.stringify(firstActive)} after ${JSON.stringify(secondActive)}`,
    );
    checks.activeIsTheNeedle = expect(
      secondActive.text?.toLowerCase(),
      "needle",
      `active range text ${JSON.stringify(secondActive.text)}`,
    );
    const counterAfter = await evaluate(
      client,
      `document.querySelector('.findbar__count').textContent`,
    );
    checks.counterAdvances = expect(
      counterAfter,
      `2 of ${ranges.matchCount}`,
      `counter after Enter: "${counterAfter}"`,
    );

    // 5. Cmd+F while open keeps the query (refocus, not reset).
    await evaluate(client, `window.__find.pressCmdF(document.body)`);
    await delay(300);
    checks.cmdFWhileOpenKeepsQuery = expect(
      await evaluate(client, `document.querySelector('.findbar__input').value`),
      "needle",
      "query survives a second Cmd+F",
    );
    checks.cmdFWhileOpenRefocuses = expect(
      await evaluate(
        client,
        `document.activeElement?.classList.contains('findbar__input') === true`,
      ),
      true,
      "find input refocused",
    );

    // 6. A hunk-header match paints on the separator row.
    await evaluate(client, `window.__find.pressCmdF(document.body)`);
    await delay(200);
    await evaluate(client, `window.__find.type("function tail")`);
    await delay(900);
    const hunkRanges = await evaluate(client, `window.__find.highlightTexts()`);
    checks.hunkHeaderHighlighted = expect(
      hunkRanges.hosts.includes("separator"),
      true,
      `hosts for a hunk-header query: ${JSON.stringify(hunkRanges.hosts)}, texts ${JSON.stringify(hunkRanges.texts)}`,
    );

    // Reset for the note-editor check.
    await evaluate(client, `window.__find.type("needle")`);
    await delay(600);

    // 7. Cmd+F inside a note textarea is ignored.
    await evaluate(client, `window.__find.openComposer()`);
    await delay(700);
    const composerOpen = await evaluate(
      client,
      `Boolean(document.querySelector('.comment__textarea'))`,
    );
    if (composerOpen) {
      await evaluate(client, `window.__find.closeFind()`);
      await delay(200);
      await evaluate(
        client,
        `window.__find.pressCmdF(document.querySelector('.comment__textarea'))`,
      );
      await delay(250);
      checks.cmdFDefersToNoteEditor = expect(
        await evaluate(client, `Boolean(document.querySelector('.findbar'))`),
        false,
        "find bar stayed closed while typing a note",
      );
    } else {
      checks.cmdFDefersToNoteEditor = {
        pass: false,
        detail: "could not open a composer to test against",
      };
    }

    return { checks };
  } finally {
    client?.close();
    await stopChild(chrome);
    await rm(userDataDir, { recursive: true, force: true });
  }
}

function expect(actual, wanted, detail) {
  return {
    pass: JSON.stringify(actual) === JSON.stringify(wanted),
    detail: `${detail} (got ${JSON.stringify(actual)}, wanted ${JSON.stringify(wanted)})`,
  };
}

async function evaluate(client, expression) {
  const result = await client.send("Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  if (result.exceptionDetails) {
    throw new Error(
      `evaluate failed: ${expression}\n${JSON.stringify(result.exceptionDetails)}`,
    );
  }
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
        throw new Error("Unhandled mock invoke: " + cmd);
      }
    };

    function setNativeValue(input, value) {
      const setter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype, "value").set;
      setter.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    }

    function collectRanges(name) {
      const highlight = CSS.highlights.get(name);
      const out = [];
      if (!highlight) return out;
      highlight.forEach((range) => out.push(range));
      return out;
    }

    function hostName(range) {
      let node = range.startContainer;
      while (node && node.nodeType !== 1) node = node.parentNode;
      let element = node;
      while (element) {
        if (element.classList?.contains("file__path")) return "file__path";
        if (element.hasAttribute?.("data-separator")) return "separator";
        if (element.hasAttribute?.("data-line-index")) return "line";
        element = element.parentElement ?? element.getRootNode?.()?.host;
      }
      return "unknown";
    }

    window.__find = {
      pressCmdF(target) {
        (target ?? document.body).dispatchEvent(new KeyboardEvent("keydown", {
          key: "f", metaKey: true, bubbles: true
        }));
      },
      pressEnter() {
        const input = document.querySelector(".findbar__input");
        input.dispatchEvent(new KeyboardEvent("keydown", {
          key: "Enter", bubbles: true
        }));
      },
      closeFind() {
        const button = document.querySelector('.findbar [title="Close search"]');
        button?.click();
      },
      type(value) {
        setNativeValue(document.querySelector(".findbar__input"), value);
      },
      highlightTexts() {
        const all = collectRanges("diffdesk-find");
        const active = collectRanges("diffdesk-find-active");
        const ranges = [...all, ...active];
        return {
          total: ranges.length,
          matchCount: Number(
            (document.querySelector(".findbar__count").textContent
              .match(/of (\\d+)/) ?? [0, 0])[1]),
          texts: ranges.map((range) => range.toString()),
          hosts: Array.from(new Set(ranges.map(hostName)))
        };
      },
      activeInfo() {
        const [range] = collectRanges("diffdesk-find-active");
        if (!range) return { key: null, text: null };
        // Shiki splits a line into per-token text nodes, so the start container's
        // own text is not unique. Walk up to the row and key on its line number.
        let element = range.startContainer;
        while (element && element.nodeType !== 1) element = element.parentNode;
        while (element && !element.hasAttribute?.("data-line-index")) {
          element = element.parentElement ?? element.getRootNode?.()?.host;
        }
        const row = element?.getAttribute?.("data-line-index") ?? "no-row";
        const lineText = element?.textContent ?? "";
        return {
          key: row + "|" + lineText.trim() + "|" + range.startOffset,
          text: range.toString()
        };
      },
      openComposer() {
        const container = document.querySelector("diffs-container");
        const gutter = container?.shadowRoot?.querySelector(
          "[data-gutter] [data-column-number]");
        if (!gutter) return false;
        for (const type of ["pointerdown", "mousedown", "pointerup", "mouseup", "click"]) {
          gutter.dispatchEvent(new MouseEvent(type, {
            bubbles: true, composed: true, cancelable: true, button: 0
          }));
        }
        return true;
      }
    };
  })();`;
}

async function startVite(appDir) {
  const port = await freePort();
  const url = `http://127.0.0.1:${port}/`;
  const child = spawn(
    path.join(appDir, "node_modules", ".bin", "vite"),
    ["--host", "127.0.0.1", "--port", String(port)],
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
    waitForHttp(url, 20000),
    new Promise((_, reject) => {
      child.once("exit", (code) => {
        reject(new Error(`Vite exited with ${code}\n${output}`));
      });
    }),
  ]);

  return { url, stop: () => stopChild(child) };
}

function connectCdp(wsUrl) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(wsUrl);
    const pending = new Map();
    let id = 0;
    ws.onopen = () => {
      resolve({
        close: () => ws.close(),
        send(method, params = {}) {
          const messageId = ++id;
          ws.send(JSON.stringify({ id: messageId, method, params }));
          return new Promise((resolveCommand, rejectCommand) => {
            pending.set(messageId, {
              resolve: resolveCommand,
              reject: rejectCommand,
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
        if (message.error) command.reject(new Error(JSON.stringify(message.error)));
        else command.resolve(message.result);
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
    await delay(150);
  }
  throw new Error(`Timed out waiting for: ${expression}`);
}

function waitForHttp(url, timeoutMs) {
  const startedAt = Date.now();
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const request = http.get(url, (response) => {
        response.resume();
        resolve();
      });
      request.on("error", () => {
        if (Date.now() - startedAt > timeoutMs) reject(new Error(`timeout ${url}`));
        else setTimeout(attempt, 200);
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
          response.on("data", (chunk) => {
            body += chunk;
          });
          response.on("end", () => {
            try {
              resolve(JSON.parse(body));
            } catch (error) {
              retry(error);
            }
          });
        })
        .on("error", retry);
    };
    const retry = (error) => {
      if (Date.now() - startedAt > timeoutMs) reject(error);
      else setTimeout(attempt, 200);
    };
    attempt();
  });
}

function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function stopChild(child) {
  return new Promise((resolve) => {
    if (child.exitCode !== null || child.signalCode !== null) return resolve();
    child.once("exit", () => resolve());
    child.kill("SIGTERM");
    setTimeout(() => {
      child.kill("SIGKILL");
      resolve();
    }, 2500);
  });
}

function findChrome() {
  const candidates = [
    process.env.CHROME_PATH,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
  ].filter(Boolean);
  for (const candidate of candidates) {
    if (existsSync(candidate)) return candidate;
  }
  throw new Error("No Chrome found; set CHROME_PATH");
}

await main();
