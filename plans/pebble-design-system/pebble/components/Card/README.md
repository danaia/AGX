# Card

The building block of every layout. Four variants, one material:

- **raised** — `surface` + `shadow-raised-md`, `radius-lg`, `space-8` padding. Default.
- **hero** — `shadow-raised-lg`, `radius-xl`, `space-10` padding. One per page (the login card, a feature card).
- **inset** — `surface-sunken` + `shadow-inset-md`, `radius-md`. Nest inside a raised card for summaries and read-only groups ("Cart Summary").
- **graphite** — filled `graphite`, white text. At most one per view.

Optional `eyebrow`, `title`, `actions`. Never put a raised card on a raised card without an inset step between — stacked lifts look muddy.
