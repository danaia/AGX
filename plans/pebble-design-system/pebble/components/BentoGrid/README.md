# BentoGrid

A grid of raised tiles in unequal spans — the reference's bento layout. `BentoGrid` sets `columns` (default 4) and a minimum `rowHeight`; wrap each tile in `BentoCell` with `cols` / `rows` spans. Any Card or StatTile inside fills its cell.

Gap is `space-8` — neumorphic shadows need about 24px to breathe or neighbouring tiles' shadows merge. Mix one tall or wide tile per row; a grid of equal tiles reads as a table.
