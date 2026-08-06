// Caret measurement for the @mention popup.
//
// Elm knows the character offset of the `@` the caret sits in, but not where
// that character lands on screen. The usual trick is to build a throwaway
// mirror div and copy the field's computed style onto it. We don't have to:
// the chip highlighter already renders a mirror under every field, with the
// same metrics and the same text, so a Range over the mirror gives the answer
// directly.

function mirrorFor(fieldId) {
  return document.getElementById(`${fieldId}-mirror`);
}

// Rect of the caret slot at `index`, in viewport coordinates. Walks the
// mirror's text nodes (chips contribute their own "@handle" text, so offsets
// line up with the field's value one for one).
function caretRect(mirror, index) {
  const walker = document.createTreeWalker(mirror, NodeFilter.SHOW_TEXT);
  let consumed = 0;
  let node;

  while ((node = walker.nextNode())) {
    const length = node.nodeValue.length;
    if (consumed + length >= index) {
      const offset = index - consumed;
      const range = document.createRange();
      range.setStart(node, offset);
      range.setEnd(node, offset);

      const collapsed = range.getBoundingClientRect();
      if (collapsed.height > 0) return collapsed;

      // Some engines give a collapsed range no box. Measure the character
      // after the caret instead and use its left edge.
      if (offset < length) {
        range.setEnd(node, offset + 1);
        const rect = range.getBoundingClientRect();
        if (rect.height > 0) return rect;
      }
    }
    consumed += length;
  }

  return mirror.getBoundingClientRect();
}

export function wireMentions(app) {
  app.ports.caretQuery.subscribe(({ fieldId, index }) => {
    // Wait a frame: Elm has just re-rendered the mirror with the new text.
    requestAnimationFrame(() => {
      const mirror = mirrorFor(fieldId);
      if (!mirror) return;

      const rect = caretRect(mirror, index);
      app.ports.caretPos.send({
        fieldId,
        x: rect.left,
        y: rect.bottom,
        lineTop: rect.top,
        // Elm sizes the popup, so it does the edge clamping. It just can't see
        // the window.
        viewWidth: window.innerWidth,
        viewHeight: window.innerHeight,
      });
    });
  });

  app.ports.setCaret.subscribe(({ fieldId, index }) => {
    requestAnimationFrame(() => {
      const field = document.getElementById(fieldId);
      if (!field) return;

      field.focus();
      field.setSelectionRange(index, index);
    });
  });
}
