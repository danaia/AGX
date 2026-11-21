# Button

A pill that lifts off the surface and presses into it when clicked.

- **raised** (default) — the everyday button: same colour as the ground, `shadow-raised-sm`, pressing swaps to `shadow-inset-sm`. "Login", "Apply", "Checkout".
- **primary** — the single `graphite` filled pill with `on-graphite` text. At most one per view, for the thing the view is for ("Learn More").
- **ghost** — text-only, for Cancel and tertiary actions.
- **danger** — raised, `danger` text. Pair with a confirm dialog.

Sizes `sm` 36 / `md` 44 / `lg` 56px. Labels are verb-first, sentence or title case, no trailing punctuation. Give buttons room: at least `space-6` from a card edge, or the shadows clip.

Props: `variant`, `size`, `icon`, `iconRight` (Icon names), `block`, plus any `<button>` attribute.
