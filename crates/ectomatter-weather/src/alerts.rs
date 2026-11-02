//! Official warnings, from whichever service issues them for the place: the
//! National Weather Service in the United States and its territories,
//! Environment and Climate Change Canada, and MeteoAlarm for 35 European
//! countries (CC BY 4.0). Everywhere else no service is asked, and the
//! weather itself stands in (see `infer` in the crate root).
//!
//! Only the warnings a device draws are read: tornado, hurricane (or extreme
//! wind) and tropical storm (or severe wind), ranked as the constants in the
//! crate root.

use crate::{Get, HURRICANE_WARNING, NO_ALERT, TORNADO_WARNING, TROPICAL_STORM_WARNING, get_json, parse_alerts};

/// Who issues warnings for a place, by its ISO country code.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Nws,
    Canada,
    /// MeteoAlarm, with the country's feed name.
    MeteoAlarm(&'static str),
}

/// MeteoAlarm's feed name for each country whose warning regions Ectomatter knows.
const METEOALARM: [(&str, &str); 35] = [
    ("AT", "austria"), ("BA", "bosnia-herzegovina"), ("BE", "belgium"), ("BG", "bulgaria"), ("CY", "cyprus"),
    ("CZ", "czechia"), ("DE", "germany"), ("DK", "denmark"), ("EE", "estonia"), ("ES", "spain"),
    ("FI", "finland"), ("FR", "france"), ("GR", "greece"), ("HR", "croatia"), ("HU", "hungary"),
    ("IE", "ireland"), ("IL", "israel"), ("IS", "iceland"), ("IT", "italy"), ("LT", "lithuania"),
    ("LU", "luxembourg"), ("LV", "latvia"), ("MD", "moldova"), ("ME", "montenegro"), ("MK", "republic-of-north-macedonia"),
    ("MT", "malta"), ("NL", "netherlands"), ("NO", "norway"), ("PL", "poland"), ("PT", "portugal"),
    ("RO", "romania"), ("RS", "serbia"), ("SE", "sweden"), ("SI", "slovenia"), ("SK", "slovakia"),
];

impl Source {
    /// The service's name, as Settings shows it.
    pub fn name(self) -> &'static str {
        match self {
            Source::Nws => "National Weather Service",
            Source::Canada => "Environment and Climate Change Canada",
            Source::MeteoAlarm(_) => "MeteoAlarm",
        }
    }
}

pub fn source(country: &str) -> Option<Source> {
    let c = country.to_ascii_uppercase();
    match c.as_str() {
        // the Weather Service covers the territories too
        "US" | "PR" | "GU" | "VI" | "AS" | "MP" => Some(Source::Nws),
        "CA" => Some(Source::Canada),
        _ => METEOALARM.iter().find(|(k, _)| *k == c).map(|(_, feed)| Source::MeteoAlarm(feed)),
    }
}

/// The most serious warning in force at a place, or None when its service
/// has none for it or cannot be reached (the caller keeps the last answer).
pub fn ask(get: Get, lat: f32, lon: f32, country: &str, now: i64) -> Option<i32> {
    match source(country)? {
        Source::Nws => {
            // (the service asks every caller to name itself: Ectomatter names itself on every request)
            let reply = get_json(get, &format!("https://api.weather.gov/alerts/active?point={lat:.4},{lon:.4}")).ok()?;
            reply.get("features").map(|_| parse_alerts(&reply))
        }
        Source::Canada => {
            let (w, s, e, n) = (lon - 0.01, lat - 0.01, lon + 0.01, lat + 0.01);
            let reply = get_json(get, &format!("https://api.weather.gc.ca/collections/weather-alerts/items?f=json&limit=200&bbox={w},{s},{e},{n}")).ok()?;
            reply.get("features").map(|_| parse_canada(&reply, lat, lon))
        }
        Source::MeteoAlarm(feed) => {
            let region = crate::regions::find(lat, lon)?;
            let reply = get_json(get, &format!("https://feeds.meteoalarm.org/api/v1/warnings/feeds-{feed}")).ok()?;
            reply.get("warnings").map(|_| parse_meteoalarm(&reply, region, now))
        }
    }
}

/// The most serious warning in force at a point, from Canada's weather-alerts
/// collection (GeoJSON, each warning with the outline of its area).
pub fn parse_canada(reply: &serde_json::Value, lat: f32, lon: f32) -> i32 {
    let mut worst = NO_ALERT;
    for f in reply["features"].as_array().into_iter().flatten() {
        let p = &f["properties"];
        if p["alert_type"].as_str() != Some("warning") || p["status_en"].as_str() == Some("ended") {
            continue;
        }
        let rank = match p["alert_name_en"].as_str().unwrap_or("").to_ascii_lowercase().as_str() {
            "tornado warning" => TORNADO_WARNING,
            "hurricane warning" => HURRICANE_WARNING,
            "tropical storm warning" => TROPICAL_STORM_WARNING,
            _ => NO_ALERT,
        };
        if rank > worst && geometry_contains(&f["geometry"], lon, lat) {
            worst = rank;
        }
    }
    worst
}

/// The most serious warning in force now for one MeteoAlarm region. Europe
/// issues no tornado or hurricane warnings as such; a red wind warning is
/// read as the hurricane, the way the Weather Service's Extreme Wind Warning
/// is, and an orange one as the tropical storm.
pub fn parse_meteoalarm(reply: &serde_json::Value, region: &str, now: i64) -> i32 {
    let mut worst = NO_ALERT;
    for w in reply["warnings"].as_array().into_iter().flatten() {
        let a = &w["alert"];
        if a["msgType"].as_str() == Some("Cancel") || a["status"].as_str().is_some_and(|s| s != "Actual") {
            continue;
        }
        for info in a["info"].as_array().into_iter().flatten() {
            let here = info["area"].as_array().into_iter().flatten().any(|ar| {
                ar["geocode"].as_array().into_iter().flatten().any(|g| g["valueName"] == "EMMA_ID" && g["value"].as_str() == Some(region))
            });
            if !here {
                continue;
            }
            let from = info["onset"].as_str().or(info["effective"].as_str()).and_then(rfc3339).unwrap_or(i64::MIN);
            let to = info["expires"].as_str().and_then(rfc3339).unwrap_or(i64::MAX);
            if now < from || now > to {
                continue;
            }
            let param = |name: &str| {
                info["parameter"].as_array().into_iter().flatten()
                    .find(|p| p["valueName"] == name)
                    .and_then(|p| p["value"].as_str())
                    .and_then(|v| v.split(';').next())
                    .and_then(|n| n.trim().parse::<i32>().ok())
            };
            let rank = match (param("awareness_type"), param("awareness_level")) {
                (Some(1), Some(4)) => HURRICANE_WARNING,
                (Some(1), Some(3)) => TROPICAL_STORM_WARNING,
                _ => NO_ALERT,
            };
            worst = worst.max(rank);
        }
    }
    worst
}

/// Whether a GeoJSON Polygon or MultiPolygon holds a point.
pub(crate) fn geometry_contains(g: &serde_json::Value, x: f32, y: f32) -> bool {
    let ring = |r: &serde_json::Value| -> Vec<[f32; 2]> {
        r.as_array().into_iter().flatten()
            .filter_map(|p| Some([p[0].as_f64()? as f32, p[1].as_f64()? as f32]))
            .collect()
    };
    let polygon = |p: &serde_json::Value| -> Vec<Vec<[f32; 2]>> { p.as_array().into_iter().flatten().map(ring).collect() };
    match g["type"].as_str() {
        Some("Polygon") => polygon_contains(&polygon(&g["coordinates"]), x, y),
        Some("MultiPolygon") => g["coordinates"].as_array().into_iter().flatten().any(|p| polygon_contains(&polygon(p), x, y)),
        _ => false,
    }
}

/// Inside the outer ring and outside every hole.
pub(crate) fn polygon_contains(rings: &[Vec<[f32; 2]>], x: f32, y: f32) -> bool {
    let inside = |r: &[[f32; 2]]| {
        let mut c = false;
        let n = r.len();
        for i in 0..n {
            let (a, b) = (r[i], r[(i + n - 1) % n]);
            if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
                c = !c;
            }
        }
        c
    };
    match rings.split_first() {
        Some((outer, holes)) => inside(outer) && !holes.iter().any(|h| inside(h)),
        None => false,
    }
}

/// Seconds since 1970 for a time such as "2026-09-24T10:25:00+02:00" or "…Z".
pub fn rfc3339(s: &str) -> Option<i64> {
    let (date, rest) = s.split_once('T')?;
    let mut d = date.split('-').map(|v| v.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (clock, offset) = match rest.find(['+', '-', 'Z']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "Z"),
    };
    let mut c = clock.split(':').map(|v| v.split('.').next().and_then(|v| v.parse::<i64>().ok()));
    let (hh, mm, ss) = (c.next()??, c.next()??, c.next().flatten().unwrap_or(0));
    let off = if offset == "Z" { 0 } else {
        let sign = if offset.starts_with('-') { -1 } else { 1 };
        let mut o = offset[1..].split(':').map(|v| v.parse::<i64>().ok());
        sign * (o.next()?? * 3600 + o.next().flatten().unwrap_or(0) * 60)
    };
    // days from the civil date (Howard Hinnant's algorithm)
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss - off)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_the_service_for_a_country() {
        assert_eq!(source("us"), Some(Source::Nws));
        assert_eq!(source("PR"), Some(Source::Nws));
        assert_eq!(source("CA"), Some(Source::Canada));
        assert_eq!(source("DE"), Some(Source::MeteoAlarm("germany")));
        assert_eq!(source("JP"), None);
        assert_eq!(source(""), None);
    }

    #[test]
    fn reads_times() {
        assert_eq!(rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339("2026-09-24T10:25:00+02:00"), rfc3339("2026-09-24T08:25:00Z"));
        assert_eq!(rfc3339("2026-09-29T08:21:45.383Z"), Some(1790670105));
        assert_eq!(rfc3339("nonsense"), None);
    }

    #[test]
    fn reads_canadas_warnings_for_the_point_only() {
        let square = |x: f32, y: f32| json!({ "type": "Polygon", "coordinates": [[[x, y], [x + 1.0, y], [x + 1.0, y + 1.0], [x, y + 1.0], [x, y]]] });
        let f = |name: &str, status: &str, g: serde_json::Value| json!({ "geometry": g, "properties": { "alert_type": "warning", "alert_name_en": name, "status_en": status } });
        let reply = json!({ "features": [
            f("tornado warning", "active", square(10.0, 10.0)),
            f("hurricane warning", "ended", square(-80.5, 44.5)),
            f("tropical storm warning", "active", square(-80.5, 44.5)),
            f("rainfall warning", "active", square(-80.5, 44.5)),
        ]});
        assert_eq!(parse_canada(&reply, 45.0, -80.0), TROPICAL_STORM_WARNING);
        assert_eq!(parse_canada(&reply, 10.5, 10.5), TORNADO_WARNING);
        assert_eq!(parse_canada(&reply, 0.0, 0.0), NO_ALERT);
    }

    #[test]
    fn reads_meteoalarm_wind_warnings_in_force() {
        let warn = |region: &str, kind: &str, level: &str, onset: &str, expires: &str| json!({ "alert": {
            "msgType": "Alert", "status": "Actual",
            "info": [{ "onset": onset, "expires": expires,
                "area": [{ "geocode": [{ "valueName": "EMMA_ID", "value": region }] }],
                "parameter": [{ "valueName": "awareness_level", "value": level }, { "valueName": "awareness_type", "value": kind }] }] } });
        let now = rfc3339("2026-09-24T12:00:00Z").unwrap();
        let reply = json!({ "warnings": [
            warn("DE094", "1; Wind", "3; orange; Severe", "2026-09-24T10:00:00Z", "2026-09-24T18:00:00Z"),
            warn("DE094", "1; Wind", "4; red; Extreme", "2026-09-25T10:00:00Z", "2026-09-25T18:00:00Z"),
            warn("DE094", "3; Thunderstorm", "4; red; Extreme", "2026-09-24T10:00:00Z", "2026-09-24T18:00:00Z"),
            warn("DE179", "1; Wind", "4; red; Extreme", "2026-09-24T10:00:00Z", "2026-09-24T18:00:00Z"),
        ]});
        assert_eq!(parse_meteoalarm(&reply, "DE094", now), TROPICAL_STORM_WARNING);
        assert_eq!(parse_meteoalarm(&reply, "DE179", now), HURRICANE_WARNING);
        assert_eq!(parse_meteoalarm(&reply, "FR001", now), NO_ALERT);
    }
}
