# light-studio-astro

The astronomy behind Light Studio's **Planner** view: where the Sun, the Moon and the Milky Way's
core are, when they rise and set, how long the twilights last, when the golden and blue hours are,
how much of the Moon is lit, which meteor showers are active, which nights are dark enough for the
core, and the lens arithmetic that goes with it. Pure computation: no I/O, no clock, no network.

- **`sun`:** apparent solar coordinates (about 0.01°) and the equatorial → horizontal conversion
  every body uses. Azimuth is clockwise from north (0 = N, 90 = E); altitude is geometric.
- **`moon`:** Meeus's lunar theory cut to 30 longitude/distance and 14 latitude terms (about
  0.05°, a minute or two of rise and set), parallax, illuminated fraction, the eight phase names,
  and `next_phase` (new / quarter / full moon to the second).
- **`sky`:** the Galactic Centre (Sagittarius A*, J2000) carried to the date by precession, and a
  table of twelve meteor showers (IMO peak, activity span, typical ZHR, radiant): approximate
  figures for planning, not an ephemeris.
- **`plan`:** `plan_day` finds rise, set, transit, the three twilights, golden and blue hours, and
  the dark stretches in which the core is up, for the 24 hours from a local midnight. Everything is
  found by sampling a yes/no question every five minutes and bisecting each change, so polar days,
  polar nights and events around midnight need no special cases. `score_nights` ranks nights for
  the core by minutes of core in a dark sky, with and without the Moon.
  `night_profile` lays a whole night (noon to noon) out as stretches (Sun below the horizon, in the
  dark, Moon up, core up in the dark) for the year chart, and `dark_core_windows` gives the night's
  core window as one stretch across midnight. `moon::principal_phases` lists the new, quarter and
  full moons between two times.
- **`optics`:** depth of field and hyperfocal distance, the NPF star-trail exposure (with the
  declination correction), the 500 rule, angle of view, sensor sizes with their circle of
  confusion, star-trail length in pixels, neutral density shutter times and time-lapse photo
  counts and intervals.
- **`time`:** Julian day, civil dates, European summer time, and a best-effort UTC offset for a
  place (Europe by its three clock zones, elsewhere the nearest hour of longitude). It cannot know
  every border or law, so the UI lets a person set the offset by hand.

Times are Unix seconds in UTC (`f64`); the caller converts to local time with a UTC offset.

**Never panics on hostile input:** NaN and out-of-range numbers are brought into range or find
nothing, loops are bounded.

Sources: Jean Meeus, *Astronomical Algorithms* 2nd ed. (ch. 12, 13, 21, 25, 47, 48: formulas, not
code); Howard Hinnant's date algorithms (public domain); the NPF rule (Frédéric Michaud, 2010);
IMO meteor shower calendar (facts); Galactic Centre position from the IAU Galactic coordinate
system. Verified against Meeus's worked examples (25.a, 47.a) and published moon phase times.
