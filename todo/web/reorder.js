// Pointer-based task dragging for the Elm task list.
//
// Native HTML5 drag events are inconsistent in the macOS webview, so the
// handle uses pointer events and sends the current target back through Elm's
// ports. Elm remains responsible for changing and persisting task order.

function taskAtPoint(event) {
  const element = document.elementFromPoint(event.clientX, event.clientY);
  return element?.closest?.(".task") ?? null;
}

function dropEndAtPoint(event) {
  const element = document.elementFromPoint(event.clientX, event.clientY);
  return element?.closest?.(".task-drop-end") ?? null;
}

function locationAtPoint(event) {
  const endZone = dropEndAtPoint(event);
  const afterId = endZone?.dataset.dropAfterId;
  if (afterId) return { afterId, key: `after:${afterId}` };

  const targetId = taskAtPoint(event)?.dataset.taskId;
  if (targetId) return { targetId, key: `before:${targetId}` };

  return null;
}

export function wireTaskReordering(app) {
  let drag = null;
  let currentLocation = null;
  let lastLocationKey = null;

  const endDrag = (location = null) => {
    if (!drag) return;

    if (location?.afterId) {
      app.ports.taskDroppedAfter.send(location.afterId);
    } else if (location?.targetId) {
      app.ports.taskDropped.send(location.targetId);
    } else {
      app.ports.taskDragEnded.send(null);
    }

    document.body.classList.remove("is-reordering");
    drag = null;
    currentLocation = null;
    lastLocationKey = null;
  };

  document.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;

    const handle = event.target.closest?.(".drag-handle");
    const task = handle?.closest?.(".task");
    const taskId = task?.dataset.taskId;
    if (!taskId) return;

    event.preventDefault();
    drag = { id: taskId, pointerId: event.pointerId };
    currentLocation = null;
    lastLocationKey = null;
    document.body.classList.add("is-reordering");
    app.ports.taskDragStarted.send(taskId);
  });

  document.addEventListener("pointermove", (event) => {
    if (!drag || event.pointerId !== drag.pointerId) return;

    event.preventDefault();
    const location = locationAtPoint(event);
    if (location && location.key !== lastLocationKey) {
      currentLocation = location;
      lastLocationKey = location.key;
      if (location.afterId) {
        app.ports.taskDragOverAfter.send(location.afterId);
      } else {
        app.ports.taskDragOver.send(location.targetId);
      }
    }
  });

  document.addEventListener("pointerup", (event) => {
    if (!drag || event.pointerId !== drag.pointerId) return;

    event.preventDefault();
    endDrag(locationAtPoint(event) ?? currentLocation);
  });

  document.addEventListener("pointercancel", (event) => {
    if (drag && event.pointerId === drag.pointerId) endDrag();
  });

  document.addEventListener("keydown", (event) => {
    if (drag && event.key === "Escape") {
      event.preventDefault();
      endDrag();
    }
  });
}
