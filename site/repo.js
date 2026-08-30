(async function () {
  const params = new URLSearchParams(location.search);
  const repo = (params.get("r") || "").replace(/[^\w.\/-]/g, "");
  const nameEl = document.getElementById("rp-name");
  const scoreEl = document.getElementById("rp-score");
  const verdictEl = document.getElementById("rp-verdict");

  if (!repo || repo.split("/").length !== 2) {
    nameEl.textContent = "No repository specified";
    scoreEl.textContent = "";
    verdictEl.innerHTML = 'Open a profile from the <a href="/survival">leaderboard</a>, or audit any repo yourself: <code>re audit owner/repo</code>';
    return;
  }

  nameEl.textContent = repo;
  document.title = `${repo} — AI Code Survival Profile — Causari`;
  document.getElementById("rp-cmd").textContent = `re audit ${repo}`;

  try {
    const sources = [
      "https://raw.githubusercontent.com/croviatrust/causari/leaderboard-data/site/survival-data.json",
      "/survival-data.json",
    ];
    let data = null;
    for (const src of sources) {
      try {
        const res = await fetch(src, { cache: "no-cache" });
        if (res.ok) { data = await res.json(); break; }
      } catch (_) { /* try next */ }
    }
    if (!data) throw new Error("no data");
    const row = (data.rows || []).find(r => r.repo.toLowerCase() === repo.toLowerCase());
    if (!row) {
      scoreEl.textContent = "not yet audited";
      scoreEl.className = "rp-score none";
      verdictEl.innerHTML = `This repository is not in the weekly Atlas yet. Audit it yourself in seconds: <code>re audit ${repo}</code> — or <a href="https://github.com/croviatrust/causari/edit/main/.github/leaderboard-repos.txt" rel="noopener">add it with a one-line PR</a>.`;
      return;
    }

    const fmt = n => n.toLocaleString("en-US");
    const v = row.verified;
    if (v.introduced > 0) {
      const pct = Math.round((v.survival_rate ?? 0) * 1000) / 10;
      scoreEl.textContent = pct + "%";
      scoreEl.className = "rp-score " + (pct >= 70 ? "hi" : pct >= 40 ? "mid" : "lo");
      verdictEl.textContent = pct >= 70
        ? "Healthy: most verified AI-written lines are still alive at HEAD."
        : pct >= 40
          ? "Moderate churn: a significant share of AI-written lines has been rewritten."
          : "High churn: most AI-written lines did not survive.";
    } else {
      scoreEl.textContent = "no verified AI signal";
      scoreEl.className = "rp-score none";
      verdictEl.textContent = "No machine-readable AI authorship metadata found in git history — absence of evidence, not evidence of absence.";
    }

    document.getElementById("s-commits").textContent = fmt(row.total_commits);
    document.getElementById("s-ai").textContent = fmt(v.commits) + (row.probable.commits ? ` (+${fmt(row.probable.commits)} probable)` : "");
    document.getElementById("s-intro").textContent = fmt(v.introduced);
    document.getElementById("s-surv").textContent = fmt(v.surviving);
    document.getElementById("rp-grid").hidden = false;

    document.getElementById("rp-badge").textContent =
      `[![AI survival](https://img.shields.io/badge/AI_survival-${v.introduced > 0 ? (Math.round((v.survival_rate ?? 0) * 1000) / 10) + "%25" : "n%2Fa"}-informational)](https://causari.dev/repo?r=${repo})`;

    const meta = [];
    if (row.audited_at) meta.push(`Last audited ${new Date(row.audited_at).toUTCString()}`);
    meta.push("VERIFIED = explicit git metadata only; PROBABLE is never mixed into the score");
    meta.push(`Reproduce: re audit ${repo}`);
    document.getElementById("rp-meta").textContent = meta.join(" · ");
  } catch (e) {
    scoreEl.textContent = "data unavailable";
    scoreEl.className = "rp-score none";
    verdictEl.innerHTML = `Try the audit yourself: <code>re audit ${repo}</code>`;
  }
})();
document.getElementById("year").textContent = new Date().getFullYear();
