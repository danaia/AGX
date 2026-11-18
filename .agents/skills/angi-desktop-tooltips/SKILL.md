---
name: angi-desktop-tooltips
description: Add or review ANGI desktop tooltips using the Slint pattern from the Agent Angie OS and Project toggle.
---

# ANGI desktop tooltips

Use Agent Angie's OS and Project toggle in `crates/agx-gui/ui/agent-scope.slint` as the reference. Reuse `HoverDwell` and `HoverTip` from `crates/agx-gui/ui/hover-tip.slint`.

- Show the tooltip after 400 ms of continuous hover. Dismiss it on pointer exit and when the control activates.
- Use the shared surface with an 8 px radius, a 1 px accent border, a light theme background, and bright 12 px text. The shared component wraps text with 13 px horizontal and 11 px vertical padding.
- Place the tooltip 8 px below the control or control group. Render it above nearby content, outside any scroll viewport, and clamp its horizontal position and width to fit the panel. Keep it passive so it cannot intercept clicks.
- Give icon-only controls a concise tooltip and an accessible label. Keep their keyboard activation and focus indicator.
