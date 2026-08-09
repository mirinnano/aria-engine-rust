# Umikaze progression contract

The active release contains exactly `canonical_chapter_00` through
`canonical_chapter_10`.  Completing each active chapter writes the persistent
flag `canonical_chapter_NN_completed` (where `NN` is `00`–`10`) and may unlock
only the next active chapter.  These flags are save-owned system progression:
manual and quick story loads merge their current values instead of rolling
them back.

`canonical_chapter_10_completed` records completion of the currently shipped
Day 10 chapter only. It is **not** a declaration that the unshipped full main
route, afterstory, or Another View exists.

The following persistent flag names are reserved for future authored content:

- `canonical_main_completed`
- `canonical_afterstory_unlocked`
- `canonical_another_view_unlocked`

No current script, title surface, chapter card, or route may set or present a
reserved flag. Future content must explicitly author its route and unlock
condition before using one; saves may retain the names without making absent
content visible.
