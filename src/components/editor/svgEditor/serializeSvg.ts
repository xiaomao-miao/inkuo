/**
 * Serialize the live `<svg>` DOM back out to a string for saving. We
 * hand the result straight to `write_document`, which performs an
 * atomic file replace. The DOM already contains the wrapper groups
 * (`<g data-svg-edit-handle>`) that were added during prepare, so
 * those naturally travel along with the save — they encode the
 * translate / scale / rotate transforms the user applied.
 *
 * We do a light cleanup pass before serialization:
 *
 *   - Strip the `data-svg-edit-handle` attribute itself (it's pure
 *     metadata; the `id` we also added gets stripped too). This keeps
 *     saved SVGs looking like normal hand-authored files instead of
 *     leaking editor-internal markers.
 *   - Strip `data-svg-edit-id` (v1 didn't end up using it, but a
 *     partial refactor may have left some; safe to remove).
 *
 * We intentionally do NOT touch the inner elements' geometry attrs —
 * the whole point of the wrapper-group design is that those stay
 * pristine.
 */
export function serializeSvg(svg: SVGSVGElement): string {
  const cloned = svg.cloneNode(true) as SVGSVGElement;

  const EDIT_HANDLE_ATTR = "data-svg-edit-handle";
  const EDIT_ID_ATTR = "data-svg-edit-id";
  const wrappers = cloned.querySelectorAll(`[${EDIT_HANDLE_ATTR}]`);
  wrappers.forEach((node) => {
    node.removeAttribute(EDIT_HANDLE_ATTR);
    // Drop the auto-stamped id if it's our synthetic `e...` shape so we
    // don't pollute the saved file with meaningless identifiers.
    const id = node.getAttribute("id");
    if (id && /^e[0-9a-z]+$/.test(id)) {
      node.removeAttribute("id");
    }
  });

  // Sweep for stray data attrs we may have left during a future
  // refactor. The query is cheap and saves us from polluting the file.
  const stray = cloned.querySelectorAll("[data-svg-edit-id]");
  stray.forEach((node) => node.removeAttribute("data-svg-edit-id"));

  const serializer = new XMLSerializer();
  return serializer.serializeToString(cloned);
}
