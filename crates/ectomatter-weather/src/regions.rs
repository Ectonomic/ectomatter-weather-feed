//! Which MeteoAlarm warning region a point lies in, from the outlines in
//! data/meteoalarm-regions.json (see data/NOTICE.md), read once when first
//! needed.

use std::sync::OnceLock;

use crate::alerts::polygon_contains;

struct Region {
    code: String,
    /// polygons, each an outer ring and its holes, as [lon, lat]
    polygons: Vec<Vec<Vec<[f32; 2]>>>,
    /// lon/lat bounds, to skip most regions without testing them
    bounds: [f32; 4],
}

fn regions() -> &'static [Region] {
    static R: OnceLock<Vec<Region>> = OnceLock::new();
    R.get_or_init(|| {
        let all: serde_json::Value = serde_json::from_str(include_str!("../data/meteoalarm-regions.json")).unwrap_or_default();
        all.as_array().into_iter().flatten().filter_map(|r| {
            let polygons: Vec<Vec<Vec<[f32; 2]>>> = r["p"].as_array()?.iter().map(|p| {
                p.as_array().into_iter().flatten().map(|ring| {
                    ring.as_array().into_iter().flatten().filter_map(|pt| Some([pt[0].as_f64()? as f32, pt[1].as_f64()? as f32])).collect()
                }).collect()
            }).collect();
            let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for pt in polygons.iter().filter_map(|p| p.first()).flatten() {
                b = [b[0].min(pt[0]), b[1].min(pt[1]), b[2].max(pt[0]), b[3].max(pt[1])];
            }
            Some(Region { code: r["c"].as_str()?.to_string(), polygons, bounds: b })
        }).collect()
    })
}

/// The EMMA_ID of the region holding a point, if MeteoAlarm has one there.
pub fn find(lat: f32, lon: f32) -> Option<&'static str> {
    regions().iter()
        .filter(|r| lon >= r.bounds[0] && lon <= r.bounds[2] && lat >= r.bounds[1] && lat <= r.bounds[3])
        .find(|r| r.polygons.iter().any(|p| polygon_contains(p, lon, lat)))
        .map(|r| r.code.as_str())
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_regions() {
        let berlin = super::find(52.52, 13.405).expect("Berlin is in a region");
        assert!(berlin.starts_with("DE"), "{berlin}");
        assert!(super::find(48.8566, 2.3522).expect("Paris").starts_with("FR"));
        assert_eq!(super::find(40.71, -74.0), None); // New York
    }
}
