# Pebble

Pebble is a light, monochrome, soft-UI (neumorphic) design system: one pale grey material, one light from the top-left, and quiet grey type. Parts are not drawn with borders or colour — they are *pressed out of* or *into* the surface with paired shadows. The only accent is graphite.

Built from four reference images supplied for this system (a neumorphism bento template, a minimal login form, and a set of shop components: price-range sliders, cart summary, icon rail, category radios). Values were sampled by eye from those images and then adjusted for legibility — see *Accessibility* for what was changed and why. There is no logo; the name is set in Poppins.

## Principles

1. **One material.** Cards, buttons, rails and the page are all `surface`. Depth comes only from `shadow-raised-*` (lifted) and `shadow-inset-*` (pressed). Never give a raised part its own background colour to make it "pop".
2. **One light.** Highlight upper-left (`shadow-light`), shade lower-right (`shadow-dark`). Every shadow token already follows this; never write a shadow by hand or flip its direction.
3. **Raised acts, inset holds.** Things you press (buttons, chips, thumbs, cards you open) are raised. Things that receive or contain (inputs, tracks, wells, summaries, the selected nav item) are inset. A pressed button becomes inset.
4. **Grey first, graphite once.** Content is `ink` and `ink-muted`. `graphite` fills mark the single most important action or state in a view. Status colours (success, warning, danger) appear only as small tinted discs, dots and text — never as card backgrounds.
5. **Room to breathe.** Shadows extend ~24px. Keep at least `space-6` inside cards and `space-8` between tiles, or neighbouring shadows muddy together.

## Voice

Short, calm, literal. Labels are nouns ("Username", "Price Range"); buttons are verbs ("Login", "Apply", "Checkout", "Learn More"). Sentence or title case — the reference uses both; pick one per product and keep it. Uppercase is reserved for small captions under a rule ("THIS IS EDITABLE TEMPLATE"). No exclamation marks, no emoji.

## Colour

| Role | Tokens |
| --- | --- |
| Material | `surface` (#ebebeb), `surface-sunken` (wells), `surface-raised` (thumbs, selected segment) |
| Text | `ink` (headings, body, values), `ink-muted` (secondary, placeholders), `ink-subtle` (disabled only) |
| Lines | `line` (hairlines), `line-strong` (heading rule, radio rings) |
| Accent | `graphite` fill + `on-graphite` text, `graphite-hover`, `graphite-soft` (neutral badge/avatar) |
| Status | `success` / `warning` / `danger`, each with `-soft` fill and `-ink` text for use on it |
| Overlay | `scrim` behind dialogs, with a 6px backdrop blur |

## Type

Poppins (Google Fonts, weights 300/400/500/600) — the geometric sans in all four references. Hierarchy comes from size and weight, rarely colour:

- **Stacked headings** pair weights: a 600 line over a 300 line ("LOREM / IPSUM"), or `display` + an uppercase `heading-2` with a `rule` Divider and a `caption` under it.
- **Figures** use `numeral` — light 300 weight, tabular — so balances feel airy, not shouty.
- **Labels** are `label` (15/500), larger than body, as in the login form.

## Shape and space

Radii are large: cards `radius-lg` (28), hero cards and dialogs `radius-xl` (40), inputs and buttons `radius-pill`, inset panels `radius-md` (20). Spacing steps are 4·8·12·16·20·24·32·40·48 (`space-1` … `space-12`). Card padding is `space-8`; bento gaps are `space-8`.

## Elevation

| Level | Token | Used for |
| --- | --- | --- |
| Inset deep | `shadow-inset-md` | Summary panels, icon wells |
| Inset | `shadow-inset-sm` | Inputs, tracks, pressed buttons, current nav item |
| Inset faint | `shadow-inset-xs` | Badges, radio/checkbox wells |
| Raised faint | `shadow-raised-xs` | Thumbs, selected segment, avatars |
| Raised | `shadow-raised-sm` | Buttons, icon buttons, alerts |
| Card | `shadow-raised-md` | Cards, bento tiles, rails, tables |
| Floating | `shadow-raised-lg` | Hero card, dialog, menu, toast |
| Graphite | `shadow-graphite` | Filled graphite pills and tiles |

## Iconography

The `Icon` component holds the set: 24px grid, 1.75px stroke, round caps and joins, drawn in `currentColor` (usually `ink-muted`, `ink` when active). The reference rail used small filled glyphs; Pebble uses outlines to match the light type weight — swap in a filled set if you prefer, keeping one style per product. Icon-only buttons always carry a `label`.

## Layout: bento

Pages are grids of raised tiles in unequal spans (`BentoGrid` + `BentoCell`). Mix one tall or wide tile per row; give at most one tile the `graphite` variant. Nest an `inset` Card inside a raised one for read-only groups (Cart Summary), never raised-on-raised.

## Accessibility

Soft UI is low-contrast by nature. Pebble keeps the look and fixes what people must read or operate:

- **Text is darker than the reference.** The images set body and headings in mid grey (~#7a7a7a, under 3:1). Pebble's `ink` is 7.8:1 and `ink-muted` 5.4:1 on `surface`; placeholders use `ink-muted` too.
- **Graphite was darkened** to #6e6e6e so white labels on it reach 5.1:1.
- **Focus is an outline**, 2px `focus` (= `ink`) at 3px offset, on every control. Inputs show the ring around the well.
- **Flagged: control edges.** Input wells, switch tracks and unselected buttons are defined by soft shadow only, below the 3:1 non-text guideline — a deliberate trade for this style. Labels are always visible, radio rings use `line-strong` (3.4:1), and selected states change fill, not just shadow. For products with strict contrast requirements, add a 1px `line-strong` edge to `.pb-control` and `.pb-btn--raised`.
- Status never relies on colour alone: deltas carry arrows, badges take a dot *and* a word.

## Using the components

`components/bundle.js` defines `window.Pebble` (React 18 on the page). Load `tokens.css`, then `components/bundle.css` (which imports Poppins), then React, ReactDOM and the bundle. Class names are prefixed `pb-`. Inputs are uncontrolled by default (`defaultValue`, `defaultChecked`) and controlled when you pass `value` / `checked`. `LoginExample` and `ShopExample` are composed patterns showing the reference screens rebuilt from Pebble parts.
