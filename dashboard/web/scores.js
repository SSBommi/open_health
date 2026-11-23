// Scores panel — daily Sleep + Readiness (0–100) with the contributors behind each.
//
// Kept in its own file so upstream app.js changes merge cleanly. app.js calls
// window.renderScores(summary) after every load. Sleep comes from
// nights[].sleep_score (crates/oura-summary/src/sleep_score.rs); Readiness from
// summary.readiness (crates/oura-summary/src/readiness.rs).
// Web-only for now: see "Known gaps" in docs/clients-web-and-ios.md.
"use strict";

(function () {
  const $ = (id) => document.getElementById(id);
  const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

  const bandOf = (s) => (s >= 85 ? ["Optimal", "opt"] : s >= 70 ? ["Good", "good"] : s >= 60 ? ["Fair", "fair"] : ["Pay attention", "low"]);

  const SLEEP_LABELS = {
    duration: "Total sleep",
    efficiency: "Efficiency",
    onset_latency: "Time to fall asleep",
    waso: "Awake after falling asleep",
    awakenings: "Awakenings",
    architecture: "Deep + REM balance",
    physiology: "Heart rate + HRV",
  };

  function sleepValue(c) {
    const d = c.detail || {};
    switch (c.key) {
      case "duration": {
        const h = d.value_h ?? 0;
        return `${Math.floor(h)} h ${Math.round((h % 1) * 60)} min`;
      }
      case "efficiency": return `${d.value_pct}%`;
      case "onset_latency": return `${d.value_min} min`;
      case "waso": return `${d.value_min} min`;
      case "awakenings": return `${d.value}`;
      case "architecture":
        return [d.deep_pct != null ? `deep ${d.deep_pct}%` : null, d.rem_pct != null ? `REM ${d.rem_pct}%` : null].filter(Boolean).join(", ");
      case "physiology":
        return [d.rhr != null ? `RHR ${d.rhr}` : null, d.hrv_ms != null ? `HRV ${d.hrv_ms} ms` : null].filter(Boolean).join(", ");
      default: return "";
    }
  }

  function readinessValue(c) {
    const d = c.detail || {};
    switch (c.key) {
      case "sleep": return `Sleep score ${d.sleep_score}`;
      case "hrv": return `${d.value_ms} ms (usual ${d.baseline_ms})`;
      case "rhr": return `${d.value_bpm} bpm (usual ${d.baseline_bpm})`;
      case "temperature": {
        const v = d.deviation_c;
        return `${v > 0 ? "+" : v < 0 ? "−" : "±"}${Math.abs(v).toFixed(1)} °C from usual`;
      }
      case "sleep_balance": return `${d.avg_h} h avg of ${d.need_h} h, ${d.nights} night${d.nights === 1 ? "" : "s"}`;
      default: return "";
    }
  }

  // Longest main sleep per wake day with a sleep score — the same pairing rule the
  // day card uses (a nap never shadows the real night).
  function sleepByDay(nights) {
    const out = {};
    for (const n of nights || []) {
      const s = n.sleep_score;
      if (!s || s.score == null || !n.wake_ymd) continue;
      const prev = out[n.wake_ymd];
      if (!prev || (n.in_bed_h || 0) > (prev.in_bed_h || 0)) out[n.wake_ymd] = { ...s, in_bed_h: n.in_bed_h };
    }
    return out;
  }

  function ring(score, title) {
    const r = 34, c = 2 * Math.PI * r;
    const frac = score == null ? 0 : Math.max(0, Math.min(1, score / 100));
    const [label, kind] = score == null ? ["No data", "none"] : bandOf(score);
    return `<svg class="score-ring ${kind}" viewBox="0 0 84 84" role="img" aria-label="${esc(title)}: ${score == null ? "no data" : `${score} out of 100, ${label}`}">
      <circle class="track" cx="42" cy="42" r="${r}" />
      <circle class="arc" cx="42" cy="42" r="${r}" stroke-dasharray="${(c * frac).toFixed(1)} ${c.toFixed(1)}" transform="rotate(-90 42 42)" />
      <text x="42" y="47" text-anchor="middle" class="score-num">${score == null ? "—" : score}</text>
    </svg>`;
  }

  function contributors(list, valueOf, labels) {
    if (!list || !list.length) return "";
    const rows = list
      .slice()
      .sort((a, b) => b.weight - a.weight)
      .map((c) => {
        const [band, kind] = bandOf(c.score);
        const name = labels ? labels[c.key] || c.key : c.label;
        return `<li class="contrib ${kind}">
          <span class="c-name">${esc(name)}<span class="c-band">${esc(band)}</span></span>
          <span class="c-val">${esc(valueOf(c))}</span>
          <span class="c-bar" aria-hidden="true"><i style="width:${Math.max(4, c.score)}%"></i></span>
        </li>`;
      })
      .join("");
    return `<ul class="contribs">${rows}</ul>`;
  }

  function block(title, score, list, valueOf, labels, note) {
    const [label, kind] = score == null ? ["No data yet", "none"] : bandOf(score);
    return `<div class="score-block">
      <div class="score-head">
        ${ring(score, title)}
        <div>
          <h3 class="score-title">${esc(title)}</h3>
          <p class="score-label ${kind}">${esc(label)}</p>
          ${note ? `<p class="score-note">${esc(note)}</p>` : ""}
        </div>
      </div>
      ${contributors(list, valueOf, labels)}
    </div>`;
  }

  function trend(days, sleep, ready) {
    const last = days.slice(0, 14).reverse();
    if (last.length < 2) return "";
    const bar = (v, cls) => `<i class="${cls}" style="height:${v == null ? 0 : Math.max(4, v)}%"></i>`;
    const cols = last
      .map((d) => {
        const s = sleep[d]?.score ?? null, r = ready[d]?.score ?? null;
        const md = d.slice(5).replace("-", "/");
        return `<li title="${md}: sleep ${s ?? "—"}, readiness ${r ?? "—"}"><span class="bars">${bar(s, "b-sleep")}${bar(r, "b-ready")}</span><span class="t-day">${md}</span></li>`;
      })
      .join("");
    const table = last
      .map((d) => `<tr><th scope="row">${d}</th><td>${sleep[d]?.score ?? "—"}</td><td>${ready[d]?.score ?? "—"}</td></tr>`)
      .join("");
    return `<div class="score-trend">
      <p class="trend-key"><i class="b-sleep"></i>Sleep <i class="b-ready"></i>Readiness <span class="muted">· last ${last.length} days</span></p>
      <ol class="trend-bars" aria-hidden="true">${cols}</ol>
      <table class="sr-only"><caption>Daily scores</caption><thead><tr><th scope="col">Day</th><th scope="col">Sleep</th><th scope="col">Readiness</th></tr></thead><tbody>${table}</tbody></table>
    </div>`;
  }

  let STATE = { days: [], idx: 0, sleep: {}, ready: {} };

  function draw() {
    const box = $("scores");
    const nav = $("scores-nav");
    const { days, idx, sleep, ready } = STATE;
    if (!days.length) {
      nav.hidden = true;
      box.innerHTML = `<p class="empty">No scores yet. Wear the ring overnight, then sync. Sleep scores from the first night; Readiness gets sharper over 14 nights.</p>`;
      return;
    }
    nav.hidden = false;
    const day = days[idx];
    $("scores-day").textContent = new Date(day + "T12:00:00").toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
    $("scores-prev").disabled = idx >= days.length - 1;
    $("scores-next").disabled = idx <= 0;
    const s = sleep[day], r = ready[day];
    const rNote = r && r.provisional
      ? `Learning your baseline: ${r.baseline_nights} of 14 nights`
      : null;
    box.innerHTML =
      `<div class="score-pair">` +
      block("Sleep", s?.score ?? null, s?.components, sleepValue, SLEEP_LABELS, null) +
      block("Readiness", r?.score ?? null, r?.components, readinessValue, null, rNote) +
      `</div>` +
      trend(days, sleep, ready);
  }

  function step(delta) {
    const n = STATE.idx + delta;
    if (n < 0 || n >= STATE.days.length) return;
    STATE.idx = n;
    draw();
  }

  window.renderScores = function (d) {
    const sleep = sleepByDay(d.nights);
    const ready = {};
    for (const r of d.readiness || []) ready[r.wake_ymd] = r;
    const days = [...new Set([...Object.keys(sleep), ...Object.keys(ready)])].sort().reverse();
    const keep = STATE.days[STATE.idx];
    STATE = { days, sleep, ready, idx: Math.max(0, days.indexOf(keep)) };
    draw();
  };

  document.addEventListener("DOMContentLoaded", () => {
    $("scores-prev")?.addEventListener("click", () => step(1));
    $("scores-next")?.addEventListener("click", () => step(-1));
  });
  // the script tag sits at the end of <body>, so wire now too if the DOM is ready
  if (document.readyState !== "loading") {
    $("scores-prev")?.addEventListener("click", () => step(1));
    $("scores-next")?.addEventListener("click", () => step(-1));
  }
})();
