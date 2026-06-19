# HyprBoard Context

Domain language for HyprBoard, a PureRef/BeeRef-style reference-image board. This file names the seams the architecture review depends on; keep it in sync as terms are sharpened.

## Language

### Board model

**Board item** (`BoardItem`):
A single placed thing on the board — an image or a styled text note.
_Avoid_: object, element, node.

**Connector**:
A straight line joining two **board items**, referenced by stable `ItemId`.
_Avoid_: edge, link, arrow.

**Transform**:
An item's position, rotation, and scale on the board.
_Avoid_: matrix, placement.

### Storage & exchange

**Item record** (`ItemRecord`):
A storage-shaped, variant-split snapshot of one **board item** in primitives (floats, `u32` colors, byte blob) — the neutral middle representation between a **board item** and a stored row.
_Avoid_: DTO, row, model, RawRow.

**Codec** (`src/codec.rs`):
The module owning `ItemRecord ↔ BoardItem` conversion plus the color primitive↔domain pair. Storage-agnostic: knows nothing of SQLite or egui.
_Avoid_: serializer, mapper, converter.

**Persistence** (`src/persistence.rs`):
The storage adapter mapping SQL columns ↔ **item record**. Owns the schema, migrations, legacy-label handling, and the flat-table `NULL` threading — but no domain assembly.
_Avoid_: store, repository, DAO.

## Relationships

- A **board item** is either an image or a text note; a **codec** turns one into exactly one **item record** and back.
- An **item record** is storage-shaped: the depth (Transform assembly, color decode, crop rect, dimension probe) lives in the **codec**, not in **persistence**.
- **Persistence** maps an **item record** to/from one SQL row; the union-table `NULL`s are a **persistence** artifact, never a **codec** concern.
- z_order is list-positional and assigned by **persistence** (`enumerate`), so it is not part of an **item record**.
- A **connector** maps directly to its own table (no **item record** — it is already flat).
- `.bee` (BeeRef) import is a one-way foreign mapping in `bee_import.rs`; it builds **board items** directly and does **not** go through the **codec**.

## Example dialogue

> **Dev:** "When I add `letter_spacing` to a text note, where do I touch storage?"
> **Maintainer:** "The field list lives on the **item record**. You add it to `TextRecord` and its `ItemRecord ↔ BoardItem` arm in the **codec** — that's one place, fully unit-tested without a database. Then **persistence** binds the new column. The codec test catches a round-trip mistake; the SQLite test only proves the column wiring."

## Flagged ambiguities

- "codec" earlier conflated serialization (persistence) with rasterization (clipboard/PDF export). Resolved: **codec** is board-item↔record only. Turning an item into *pixels* is a separate rendering seam (image-render module), not part of this context.
- "RawRow" was persistence's flat all-`Option` column mirror — it is superseded by the variant-split **item record** and should not be reintroduced.
