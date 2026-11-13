//! More about the Sun and the Moon: their size in the sky and distance, the Moon's age, the four seasons and
//! the Moon's nearest and farthest points of the year.

use light_studio_astro::info::{self, Apsis};

use super::*;

impl Page<'_> {
    /// The Sun's size, distance, and the day's equinoxes and solstices.
    pub(super) fn sun_facts(&mut self, ui: &mut Ui) {
        let c = self.c;
        let at = c.start + self.minute * 60.0;
        let (diameter, dist) = info::sun_size(at);
        subhead(ui, "The Sun");
        row(ui, "sunfacts:size", "Apparent Size", &format!("{:.3}°", diameter), None);
        row(ui, "sunfacts:dist", "Distance", &format!("{:.0} km", dist), None);
        let h = self.skies.sun;
        row(
            ui,
            "sunfacts:ratio",
            "Shadow Ratio",
            &light_studio_astro::align::shadow(1.0, h).map_or_else(|| "—".to_string(), |(k, _)| format!("1 : {k:.2}")),
            None,
        );
        subhead(ui, "Seasons");
        let names = ["March Equinox", "June Solstice", "September Equinox", "December Solstice"];
        for (i, (name, t)) in names.iter().zip(info::seasons(c.date.year)).enumerate() {
            let text = t.map_or_else(|| "—".to_string(), |t| format!("{} · {}", short_date(Date::from_unix(t, c.offset)), clock(t, c.offset)));
            row(ui, &format!("season:{i}"), name, &text, None);
        }
    }

    /// The Moon's age, size and distance, and its perigees and apogees round the day shown.
    pub(super) fn moon_facts(&mut self, ui: &mut Ui, rt: &mut Runtime) {
        let t = self.t;
        let c = self.c;
        let at = c.start + self.minute * 60.0;
        let (diameter, dist) = info::moon_size(at);
        subhead(ui, "The Moon");
        row(ui, "moonfacts:age", "Age", &format!("{:.1} {}", info::moon_age_days(at), crate::i18n::tr("days")), None);
        row(ui, "moonfacts:size", "Apparent Size", &format!("{:.3}°", diameter), None);
        row(ui, "moonfacts:dist", "Distance", &format!("{:.0} km", dist), None);
        subhead(ui, "Moon Distances");
        if rt.apsides.as_ref().is_none_or(|(y, _)| *y != c.date.year) {
            let a = unix_year_start(c.date.year);
            rt.apsides = Some((c.date.year, info::lunar_apsides(a, a + 366.0 * DAY)));
        }
        let list: Vec<Apsis> = rt.apsides.as_ref().map(|(_, l)| l.clone()).unwrap_or_default();
        for a in list.iter() {
            let name = if a.perigee { "Perigee" } else { "Apogee" };
            let super_moon = a.perigee && info::is_supermoon(a.dist_km);
            let (cell, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
            let p = ui.painter();
            p.circle_filled(
                pos2(cell.left() + 4.0, cell.center().y),
                3.0,
                if a.perigee { super::skylayers::SUNSET } else { super::skylayers::MOONSET },
            );
            let when = format!("{} {}", short_date(Date::from_unix(a.unix, c.offset)), clock(a.unix, c.offset));
            p.text(
                pos2(cell.left() + 14.0, cell.center().y),
                Align2::LEFT_CENTER,
                format!("{} · {when}", crate::i18n::tr(name)),
                t.font(12.0),
                t.text_dim,
            );
            let km = format!("{:.0} km{}", a.dist_km, if super_moon { " ★" } else { "" });
            p.text(pos2(cell.right(), cell.center().y), Align2::RIGHT_CENTER, km, t.font(12.0), t.text);
        }
        note(ui, crate::i18n::tr("★ A perigee closer than 361,000 km: a full or new moon then is a supermoon."));
    }
}

fn unix_year_start(year: i32) -> f64 {
    Date::new(year, 1, 1).unix_midnight_utc()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_year_starts_at_midnight_utc_on_the_first_of_january() {
        assert_eq!(unix_year_start(1970), 0.0);
        assert_eq!(unix_year_start(2000), 946_684_800.0);
        assert_eq!(unix_year_start(2001) - unix_year_start(2000), 366.0 * 86_400.0);
    }
}
