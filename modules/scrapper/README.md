# Power Scrapper (hammer)

Your Power Scrapper golem pulls against the Snow Crows bench
([build](https://snowcrows.com/builds/raids/engineer/power-scrapper-hammer),
[bench log](https://dps.report/55xd-20260422-192633_golem)). Served by the checker at
http://127.0.0.1:8771/scrapper (`run.bat`).

| File | What it does |
|---|---|
| `crates/scrapper/src/lib.rs` | The analysis: `compare()`, `summary_row()` (history), `logs()`, `icons()`. |
| `grid.html` | The default page at `/scrapper`: a 640 px panel, one row per cycle. |
| `splits.html` | The older splits view, `/scrapper/splits`. |
| `bench.html` | The bench rotation as a cheat sheet, `/scrapper/bench`. |
| `crates/scrapper/src/panel.rs` | What the in-game addon draws (`/api/scrapper/panel`). Its judging mirrors `grid.html`: change both together. |
| `crates/scrapper/src/sheet.rs` | The bench cheat sheet: `/api/scrapper/bench` for `bench.html`, `/api/scrapper/sheet` for the addon. |
| `cache/` | The bench log (`bench.json`, downloaded once), icons and one summary per pull. Not in git. |

Your logs: golem logs where a Scrapper (elite 43) cast Electro-whirl or Shrapnel Grenade.

## The loop

- **The anchor:** the 20 s skills (Freeze and Poison Grenade, Medic Gyro, Big Ol' Bomb, Magnetic Bomb,
  Thunderclap; 16 s with alacrity) should keep rolling. Recharge counts from the end of the cast: the
  bench recasts them 16.12 to 16.36 s after the previous cast ended, never under 16. Recharges are the
  GW2 API's (`cd()`).
- **A cycle** is three mini loops, each from one Electro-whirl to the next: `core` (Electro-whirl
  through Shrapnel Grenade), then `fill`. In the mini loop holding Medic Gyro, Big Ol' Bomb or Magnetic
  Bomb everything after Shrapnel is `burst` (the bomb burst, Thunderclap, Mine Field), and that mini
  loop closes the cycle.
- **Columns:** bomb burst, nade filler, other filler. In cycle 0 the opener takes the bomb burst column.
- Your cycle k is compared with bench cycle k; past the bench's last full cycle, the one two earlier
  (Function Gyro and Grenade Barrage, 25 s, alternate between cycles). Core and burst are matched in
  order (`difflib`), fill as a set; a filler cast in another whirl of the same cycle is paired as
  "moved" instead of missed plus extra.

## How a cast is judged

- **Slot:** start to the next cast's start against the bench's at the same point, the next skill's
  cooldown not counted; more than 50 ms late flags it.
- **20 s skills:** marked only when late beyond the cycle's drift: more than 0.4 s over the least late
  20 s skill of the cycle, after taking off how much later the bench casts that skill than its own
  least late one. Lateness from a longer loop belongs to the cycle, not to every skill.
- **Hits:** each cast's hits until the skill's next cast against `hits()`, the usual count (Electro-whirl
  2, Shrapnel 3, Freeze 3, Poison 3, Barrage 6, Fire Bomb 4, Thunderclap 5, Mine Field 5, everything
  else 1). Fewer gets a red inner border. Casts in the last 4 s of a pull are not judged.
- **Cancelled autos** are left out on both sides: an auto that never landed costs only its time. Any
  other skill cancelled before it landed stays and shows as missed hits.
- Kit swaps and instants have no animation in your log, so zero-length casts are dropped on both sides.

## The opener

The log starts at the first hit, so casts before it have no animation start. `precasts()` takes a
cast still running at the start from its end (Thunderclap) and places any skill that hits before its
first logged cast at its first hit minus its usual cast-to-first-hit time (bomb fuses: Big Ol' Bomb
3 s, Magnetic 2, Galvanic and Fire Bomb 1). Mines only hit when detonated, so pre-placed mines go
first: Throw Mine, Mine Field, Big Ol' Bomb, Magnetic, Fire Bomb, Galvanic, Thunderclap. Fire Bomb
hits 4 times, 1 s apart; fewer in the first seconds means its missing tick was the hit that started
the fight. The bench (Elite Insights JSON, no hit times) gets the same set from its hit counts
(`bench_precasts()`; order and times there are a guess).

## Buffs

- **Relic of Bloodstone:** the 4th blast finisher in a field gives Fervor, +7% strike for 8 s. Per
  cycle: when it came up and which 20 s skills it covered.
- **Object in Motion:** +5% strike per boon among stability, swiftness and superspeed, compounding.
  Average bonus over the pull, superspeed uptime and its gaps.

## Findings so far (38 pulls, October 2026)

- Cycles run 17.1 to 17.9 s against the bench's 16.7 to 17.0: the cycle length is how late the 20 s
  skills come after ready.
- Mine Field lands 4 of 5 mines most of the time (34 casts short; the bench gets 5).
- Galvanic Bomb cancelled at 0.2 to 0.37 s, never placed (12 times); its full cast is about 0.48 s.
- Grenades almost always land all their hits.
