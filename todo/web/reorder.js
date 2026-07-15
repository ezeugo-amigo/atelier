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

export function wireTaskReordering(app) {
  let drag = null;
  let lastTargetId = null;

  const endDrag = ({ targetId = null, afterId = null } = {}) => {
    if (!drag) return;

    if (afterId) {
      app.ports.taskDroppedAfter.send(afterId);
    } else if (targetId) {
      app.ports.taskDropped.send(targetId);
    } else {
      app.ports.taskDragEnded.send(null);
    }

    document.body.classList.remove("is-reordering");
    drag = null;
    lastTargetId = null;
  };

  document.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;

    const handle = event.target.closest?.(".drag-handle");
    const task = handle?.closest?.(".task");
    const taskId = task?.dataset.taskId;
    if (!taskId) return;

    event.preventDefault();
    drag = { id: taskId, pointerId: event.pointerId };
    lastTargetId = taskId;
    document.body.classList.add("is-reordering");
    app.ports.taskDragStarted.send(taskId);
  });

  document.addEventListener("pointermove", (event) => {
    if (!drag || event.pointerId !== drag.pointerId) return;

    event.preventDefault();
    const endZone = dropEndAtPoint(event);
    const afterId = endZone?.dataset.dropAfterId;
    if (afterId && `after:${afterId}` !== lastTargetId) {
      lastTargetId = `after:${afterId}`;
      app.ports.taskDragOverAfter.send(afterId);
      return;
    }

    const targetId = taskAtPoint(event)?.dataset.taskId;
    if (targetId && targetId !== lastTargetId) {
      lastTargetId = targetId;
      app.ports.taskDragOver.send(targetId);
    }
  });

  document.addEventListener("pointerup", (event) => {
    if (!drag || event.pointerId !== drag.pointerId) return;

    event.preventDefault();
    const endZone = dropEndAtPoint(event);
    if (endZone?.dataset.dropAfterId) {
      endDrag({ afterId: endZone.dataset.dropAfterId });
    } else {
      endDrag({ targetId: taskAtPoint(event)?.dataset.taskId ?? null });
    }
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
