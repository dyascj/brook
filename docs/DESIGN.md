# Design

Brook should feel like a quiet room: nothing competes with what you're reading. The visual language follows T3 Code Glass, so the two apps sit well side by side.

## Layout

- The **title bar** and **sidebar** sit directly on the window. On macOS in dark mode the window is glass: the desktop shows through, blurred, behind them.
- The **workspace** (article list and reader, or Discover) floats above in a card: 12 px corners, a hairline border, 8 px from the window's edges.
- The list is 360 px wide; the reader's column is at most 680 px, centered.

## Color

One rule produces almost every color: the text color at some strength.

| Use | Strength |
| --- | --- |
| Region hairlines | 7% |
| Hover fills | 5% |
| Selection fills | 10% in dark, 6% in light |
| Wells (code, placeholders) | 6% in dark, 4% in light |
| Tertiary text and resting icons | 38% |
| Secondary text | 55% |

The single accent is `#459bf7`, for links, unread dots, focus rings, and the one primary button on a screen. Stars are gold (`#f2b53b`). Feeds without an icon get a monogram tile in one of ten calm hues, chosen by a hash of the title so each feed keeps its color.

Light mode is `#eeeef0` behind white cards with a soft shadow; dark mode is `#141416` (or glass) behind `#1c1c1f` cards, no shadow.

## Type

- The system UI face throughout (San Francisco on macOS).
- Rows: 13 to 13.5 px titles; 11.5 to 12 px metadata.
- Reader: 15, 17, or 19 px body (Settings > Reading size), 1.68 line height; titles at 1.65 times the body, semibold.
- Read articles drop to normal weight at 58% strength, so unread ones stand out without a badge war.

## Motion and feedback

Quiet and short. Rows lift to full-strength text on hover; icon buttons gain a 5% fill and press in. Work in progress is a small spinner beside what it's about, never a full-screen loader.

## The brand

**Brook** is a stream of the things you chose to read. The icon is a sunrise over a brook: a warm sun above two ripples of water, on a blue squircle. Squint and the ripples are lines of text.

- `assets/brand/icon.svg` is the source of truth for the app icon; `scripts/make-icons.sh` renders the `.icns` and Linux PNG from it.
- `assets/brand/mark.svg` is the same drawing in one color, for empty states and About.

## Writing

Short, plain, and specific. Say what happened and what to do next ("Couldn't reach that site. Check the address and your connection."). No exclamation marks, no emoji, and sentence case everywhere except proper names.
