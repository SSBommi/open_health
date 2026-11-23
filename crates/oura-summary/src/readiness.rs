//! A daily Readiness score (0–100), built from the nights the summary already computed.
//!
//! Oura's own Readiness weights are not public, so this is an openly-documented
//! stand-in with the same *ingredients*: how last night went, how your autonomic
//! markers compare with your own recent baseline, whether your skin temperature has
//! drifted, and whether you have been sleeping enough over the past week.
//!
//! ## Components (weights renormalise when one is missing)
//!
//! | key            | weight | what it measures |
//! | -------------- | ------ | ---------------- |
//! | `sleep`        | 0.25   | last night's sleep score (see `sleep_score`) |
//! | `hrv`          | 0.25   | last night's mean RMSSD vs your baseline (z-score) |
//! | `rhr`          | 0.20   | last night's lowest HR vs your baseline (z-score) |
//! | `temperature`  | 0.15   | |skin temp − baseline| in °C |
//! | `sleep_balance`| 0.15   | mean sleep over up to 7 nights vs your need |
//!
//! Baselines are **causal**: only nights that ended before the scored night count,
//! drawn from at most the previous [`BASELINE_WINDOW`] main sleeps, and they need
//! [`MIN_BASELINE_NIGHTS`] of them. Until [`FULL_BASELINE_NIGHTS`] exist the day is
//! flagged `provisional`.
//!
//! The HRV and resting-HR curves are the same ones `sleep_score` uses for its
//! physiology component, so the two scores never disagree about what "your HRV is
//! low" means.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::sleep_score::MIN_BASELINE_NIGHTS;

/// How many prior main sleeps feed a baseline.
pub const BASELINE_WINDOW: usize = 14;
/// Below this many prior nights the score is shown but flagged provisional.
pub const FULL_BASELINE_NIGHTS: usize = 14;
/// A night shorter than this (hours in bed) is a nap and never anchors a day.
const MAIN_SLEEP_MIN_H: f64 = 3.0;
/// Nights averaged for sleep balance (including the scored night).
const BALANCE_NIGHTS: usize = 7;

fn curve(x: f64, anchors: &[(f64, f64)]) -> f64 {
    if x <= anchors[0].0 {
        return anchors[0].1;
    }
    for pair in anchors.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        if x <= x1 {
            let t = if (x1 - x0).abs() < f64::EPSILON { 0.0 } else { (x - x0) / (x1 - x0) };
            return y0 + t * (y1 - y0);
        }
    }
    anchors[anchors.len() - 1].1
}

fn mean_sd(v: &[f64]) -> Option<(f64, f64)> {
    if v.len() < MIN_BASELINE_NIGHTS {
        return None;
    }
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
    Some((mean, if sd > 1e-6 { sd } else { 1.0 }))
}

/// Hours of sleep to aim for, from the same NSF bands `sleep_score` uses: the low end
/// of "recommended" plus half an hour.
fn sleep_need_h(age: f64) -> f64 {
    match age {
        a if a < 14.0 => 9.5,
        a if a < 18.0 => 8.5,
        a if a < 65.0 => 7.5,
        _ => 7.5,
    }
}

/// One main sleep, reduced to what readiness reads.
#[derive(Clone, Copy, Debug)]
struct Night {
    end_unix: f64,
    asleep_h: Option<f64>,
    sleep_score: Option<f64>,
    hrv: Option<f64>,
    rhr: Option<f64>,
    temp: Option<f64>,
}

fn label(score: f64) -> &'static str {
    match score {
        s if s >= 85.0 => "Optimal",
        s if s >= 70.0 => "Good",
        s if s >= 60.0 => "Fair",
        _ => "Pay attention",
    }
}

/// Build one readiness entry per wake day, newest first, from the summary's `nights`.
pub fn build(nights: &[Value], age: f64) -> Value {
    // Longest main sleep per wake day: a nap never shadows the real night.
    let mut by_day: BTreeMap<String, (f64, Night)> = BTreeMap::new();
    for n in nights {
        let in_bed = n["in_bed_h"].as_f64().unwrap_or(0.0);
        if in_bed < MAIN_SLEEP_MIN_H {
            continue;
        }
        let Some(day) = n["wake_ymd"].as_str() else { continue };
        let night = Night {
            end_unix: n["end_unix"].as_f64().unwrap_or(0.0),
            asleep_h: n["metrics"]["asleep_min"].as_f64().filter(|m| *m > 0.0).map(|m| m / 60.0),
            sleep_score: n["sleep_score"]["score"].as_f64(),
            hrv: n["hrv_ms"].as_f64(),
            rhr: n["rhr"].as_f64(),
            temp: n["skin_temp"].as_f64(),
        };
        let keep = by_day.get(day).is_none_or(|(h, _)| in_bed > *h);
        if keep {
            by_day.insert(day.to_string(), (in_bed, night));
        }
    }
    let mut days: Vec<(String, Night)> = by_day.into_iter().map(|(d, (_, n))| (d, n)).collect();
    days.sort_by(|a, b| a.1.end_unix.total_cmp(&b.1.end_unix));

    let need_h = sleep_need_h(age);
    let mut out: Vec<Value> = Vec::new();
    for (i, (day, night)) in days.iter().enumerate() {
        let prior: Vec<&Night> = days[..i]
            .iter()
            .rev()
            .take(BASELINE_WINDOW)
            .map(|(_, n)| n)
            .collect();
        let pick = |f: fn(&Night) -> Option<f64>| -> Vec<f64> { prior.iter().filter_map(|n| f(n)).collect() };
        let hrv_base = mean_sd(&pick(|n| n.hrv));
        let rhr_base = mean_sd(&pick(|n| n.rhr));
        let temp_base = mean_sd(&pick(|n| n.temp));

        let mut comps: Vec<(&str, &str, f64, f64, Value)> = Vec::new();

        if let Some(s) = night.sleep_score {
            comps.push(("sleep", "Previous night", s, 0.25, json!({ "sleep_score": s })));
        }
        if let (Some(hrv), Some((m, sd))) = (night.hrv, hrv_base) {
            let z = (hrv - m) / sd;
            let s = curve(z, &[(-3.0, 10.0), (-2.0, 35.0), (-1.0, 70.0), (0.0, 90.0), (1.5, 100.0)]);
            comps.push(("hrv", "HRV balance", s, 0.25, json!({
                "value_ms": hrv.round(), "baseline_ms": m.round(), "z": (z * 10.0).round() / 10.0,
            })));
        }
        if let (Some(rhr), Some((m, sd))) = (night.rhr, rhr_base) {
            let z = (rhr - m) / sd;
            let s = curve(z, &[(-2.0, 100.0), (0.0, 90.0), (1.0, 70.0), (2.0, 40.0), (3.5, 10.0)]);
            comps.push(("rhr", "Resting heart rate", s, 0.20, json!({
                "value_bpm": rhr.round(), "baseline_bpm": m.round(), "z": (z * 10.0).round() / 10.0,
            })));
        }
        if let (Some(t), Some((m, _))) = (night.temp, temp_base) {
            let dev = t - m;
            let s = curve(dev.abs(), &[(0.0, 100.0), (0.3, 90.0), (0.5, 70.0), (1.0, 35.0), (1.5, 10.0)]);
            comps.push(("temperature", "Body temperature", s, 0.15, json!({
                "deviation_c": (dev * 100.0).round() / 100.0,
            })));
        }
        let recent: Vec<f64> = days[..=i]
            .iter()
            .rev()
            .take(BALANCE_NIGHTS)
            .filter_map(|(_, n)| n.asleep_h)
            .collect();
        if !recent.is_empty() {
            let avg = recent.iter().sum::<f64>() / recent.len() as f64;
            let ratio = avg / need_h;
            let s = curve(ratio, &[(0.5, 0.0), (0.7, 40.0), (0.8, 65.0), (0.9, 85.0), (1.0, 100.0)]);
            comps.push(("sleep_balance", "Sleep balance", s, 0.15, json!({
                "avg_h": (avg * 10.0).round() / 10.0, "need_h": need_h, "nights": recent.len(),
            })));
        }

        if comps.is_empty() {
            continue;
        }
        let total_w: f64 = comps.iter().map(|c| c.3).sum();
        let score = (comps.iter().map(|c| c.2 * c.3).sum::<f64>() / total_w).round();
        out.push(json!({
            "wake_ymd": day,
            "score": score,
            "label": label(score),
            "provisional": prior.len() < FULL_BASELINE_NIGHTS,
            "baseline_nights": prior.len(),
            "components": comps.iter().map(|(k, l, s, w, d)| json!({
                "key": k,
                "label": l,
                "score": s.round(),
                "weight": ((w / total_w) * 100.0).round() / 100.0,
                "detail": d,
            })).collect::<Vec<_>>(),
        }));
    }
    out.reverse();
    json!(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn night(day: &str, end: f64, hrv: f64, rhr: f64, temp: f64, asleep_min: f64, ss: f64) -> Value {
        json!({
            "wake_ymd": day, "end_unix": end, "in_bed_h": 8.0,
            "hrv_ms": hrv, "rhr": rhr, "skin_temp": temp,
            "metrics": { "asleep_min": asleep_min },
            "sleep_score": { "score": ss },
        })
    }

    #[test]
    fn first_night_scores_on_sleep_only_and_is_provisional() {
        let r = build(&[night("2026-11-23", 1.0, 50.0, 55.0, 34.0, 450.0, 80.0)], 30.0);
        let d = &r[0];
        assert_eq!(d["provisional"], true);
        let keys: Vec<&str> = d["components"].as_array().unwrap().iter().map(|c| c["key"].as_str().unwrap()).collect();
        assert_eq!(keys, ["sleep", "sleep_balance"]);
    }

    #[test]
    fn hrv_drop_and_fever_lower_the_score() {
        let mut ns: Vec<Value> = (0..10)
            .map(|i| night(&format!("2026-11-{:02}", i + 1), i as f64, 60.0 + (i % 3) as f64, 52.0 + (i % 2) as f64, 34.0, 460.0, 85.0))
            .collect();
        ns.push(night("2026-11-11", 10.0, 61.0, 52.0, 34.0, 460.0, 85.0));
        let good = build(&ns, 30.0)[0]["score"].as_f64().unwrap();
        ns.pop();
        ns.push(night("2026-11-11", 10.0, 40.0, 60.0, 35.2, 460.0, 85.0));
        let bad = build(&ns, 30.0)[0]["score"].as_f64().unwrap();
        assert!(good >= 85.0, "good={good}");
        assert!(bad < 60.0, "bad={bad}");
    }

    #[test]
    fn baselines_never_use_future_nights_and_naps_are_ignored() {
        let mut ns = vec![
            night("2026-11-01", 1.0, 60.0, 50.0, 34.0, 450.0, 80.0),
            night("2026-11-02", 2.0, 60.0, 50.0, 34.0, 450.0, 80.0),
            night("2026-11-03", 3.0, 60.0, 50.0, 34.0, 450.0, 80.0),
            night("2026-11-04", 4.0, 60.0, 50.0, 34.0, 450.0, 80.0),
        ];
        let before = build(&ns, 30.0);
        let mut nap = night("2026-11-04", 4.5, 10.0, 90.0, 37.0, 30.0, 10.0);
        nap["in_bed_h"] = json!(0.5);
        ns.push(nap);
        ns.push(night("2026-11-05", 5.0, 20.0, 80.0, 36.0, 450.0, 80.0));
        let after = build(&ns, 30.0);
        // 2026-11-04 is unchanged by the nap and by the later bad night
        assert_eq!(before[0], after[1]);
    }
}
