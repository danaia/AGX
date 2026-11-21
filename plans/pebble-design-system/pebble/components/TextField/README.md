# TextField

A pill-shaped well pressed into the surface (`surface-sunken` + `shadow-inset-sm`) with the label above it.

- Label in the `label` style (15/22, 500) — always visible; the placeholder is an example, not the label.
- Optional leading `icon` or `prefix` (currency, units). `type="password"` adds a show/hide toggle.
- `hint` sits under the well; `error` replaces it in `danger` and draws a 1.5px danger edge.
- Focus adds a 2px `focus` ring around the well — the inset shadow alone is too faint to signal focus.

Accessibility note: like the reference, the well's edge is carried by soft shadow, below the 3:1 non-text contrast guideline. Labels are always shown so a field's position never depends on its edge.
