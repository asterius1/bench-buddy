# Condi Berserker (longbow + sword/torch)

Your Condition Berserker golem pulls against the Snow Crows bench
([build](https://snowcrows.com/builds/raids/warrior/condition-berserker-longbow-sword-torch),
[bench log](https://dps.report/YPVM-20260720-095643_golem)). Served by the checker at
http://127.0.0.1:8771/ (`run.bat`).

| File | What it does |
|---|---|
| `local.py` | The analysis: `pull()`, `compare()` (the main report), `history()`, `damage()` (opening seconds), `icons()`, `cooldown_window()`. |
| `baseline.py` | Per-skill expectations: hits a full cast lands (`EXPECT`), the bench's cast lengths, which casts fail. |
| `page.html` | The page at `/`. Opens on the Splits view; tabs for Grid, Time axis, Coach and Detail. |
| `views/*.html` | The Splits, Grid, Time axis and Coach views, served at `/view/<name>?log=<stem>` with the pull's `compare()` data inlined. |
| `cache/` | The bench log (`log.json`, downloaded once), icons and one summary per pull. Not in git. |

Your logs: every Standard Kitty Golem log from October 2026 on (older ones have no animation events)
where a Berserker (elite 18) cast Scorched Earth or Flaming Flurry.

## The loop

The rotation alternates weapons, so a loop is a sword visit and a longbow visit (swap to swap):

- **Sword:** Leap, Blaze Breaker, Rend, Rend 2, then the Flurry part (Flames of War or Flaming Flurry
  on), Sever, Gash, Hamstring. Flames of War every second loop, about 23 s apart.
- **Longbow:** open (Scorched Earth [Blood Reckoning, Scorched Earth] Fan of Fire), fillers, close.
  Blood Reckoning cycles over three loops (none, start, end), Pin Down every second loop.
- Head Butt comes right after Shattering Blow or is followed at once by Outrage (its self-stun).
- Rend is two casts (80247, then 80224).

`compare()` lines up each of your loops with the bench loop of the same number (past the bench's 8
loops, the one 6 loops earlier, since Flames of War repeats every 2nd and Blood Reckoning every 3rd).
Sections: opener; sword start (Leap, Blaze Breaker, Rend, Rend 2); Flurry part; longbow open; fillers;
close. Sections are matched in order with `difflib`, fillers as a set.

## How a cast is judged

- **Slot:** the time from a cast's start to the next real cast's start, against the bench's at the
  same point. The next cast is expected at the later of the bench's pace and that skill's ready time;
  the last cast before a swap is judged by when the first skill on the other weapon starts. More than
  50 ms late flags it.
- **Hits:** fewer hits than `EXPECT` within 1 s of the cast's end (H).
- **Buff:** Flames of War or Blood Reckoning ending before its buff landed (E).
- **Missed / extra:** a bench cast you skipped shows faded; a cast the bench has no match for is "extra".
- Section and loop times against the bench: green to +0.05 s, amber to +0.30 s, red above.

Cooldowns: Scorched Earth and Fan of Fire come back 4.0 s after their cast ends (4.00 to 4.07 s
over 81 and 62 recasts); Blood Reckoning resets Scorched Earth. A wait before a skill still on
cooldown is not counted. The Splits view has four ways to judge the fillers: "SE ready" (closing
Scorched Earth more than 0.15 s after ready is red), "SE vs bench", "whole visit" and "section time".

## Opening seconds

`damage()` compares your damage on the golem per second with the bench's (`targetDamage1S`), when
each reaches 20% of the golem (800k of 4M), the average Burning, Bleeding, Torment and Poison stacks
per 5 s window, and the bench's self buffs you lack (consumables).

## Findings so far (the first 16 to 24 pulls, October 2026)

- Flaming Flurry mostly runs to the end (1.87 s) where the bench stows it at 1.52 to 1.69 s.
- Flames of War is cut before its buff lands (about 0.48 s in) in 9 of 24 casts.
- Rend's second part is cut before it hits in 11 of 39.
- Opening: 9 to 13% behind the bench at 20 s, from fewer condition stacks (no Flames of War or Head
  Butt precast; one Scorched Earth, Fan of Fire and Blood Reckoning fewer by 20 s), not from
  damage per stack.
