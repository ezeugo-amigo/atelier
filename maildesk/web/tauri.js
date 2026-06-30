const root = document.getElementById("root");

const app = Elm.Main.init({
  node: root,
  flags: {},
});

function sendResponse(message) {
  if (app.ports.commandIn) {
    app.ports.commandIn.send(message);
  }
}

app.ports.commandOut.subscribe(async (request) => {
  const requestId = request.requestId;
  const command = request.command;
  const payload = request.payload || {};

  try {
    if (!window.__TAURI__?.core?.invoke) {
      throw new Error("Tauri invoke API is unavailable");
    }

    const data = await window.__TAURI__.core.invoke(command, payload);
    sendResponse({ requestId, command, ok: true, data, error: null });
  } catch (error) {
    sendResponse({
      requestId,
      command,
      ok: false,
      data: null,
      error: error instanceof Error ? error.message : String(error),
    });
  }
});
