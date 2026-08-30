(async function () {
  const tbody = document.querySelector("#lb tbody");
  const meta = document.getElementById("lb-meta");
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
      } catch (_) { /* try next source */ }
    }
    if (!data) throw new Error("no data source");
    const rows = (data.rows || []).filter(r => r.total_commits > 0);
    if (!rows.length) throw new Error("empty");

    // Rank: repos with verified AI lines first (by survival rate desc), then no-signal.
    const signal = rows.filter(r => r.verified.introduced > 0)
      .sort((a, b) => (b.verified.survival_rate ?? 0) - (a.verified.survival_rate ?? 0));
    const silent = rows.filter(r => r.verified.introduced === 0);

    const fmt = n => n.toLocaleString("en-US");
    const rateCell = r => {
      if (r.verified.introduced === 0) return '<span class="lb-rate none">no signal</span>';
      const pct = Math.round((r.verified.survival_rate ?? 0) * 1000) / 10;
      const cls = pct >= 70 ? "hi" : pct >= 40 ? "mid" : "lo";
      return `<span class="lb-rate ${cls}">${pct}%</span><span class="lb-bar" style="width:${Math.max(4, pct * 0.6)}px"></span>`;
    };

    tbody.innerHTML = [...signal, ...silent].map((r, i) => `
      <tr>
        <td>${i + 1}</td>
        <td><a href="https://github.com/${r.repo}" rel="noopener">${r.repo}</a></td>
        <td>${fmt(r.total_commits)}</td>
        <td>${fmt(r.verified.commits)}${r.probable.commits ? ` <span class="muted">(+${fmt(r.probable.commits)} probable)</span>` : ""}</td>
        <td>${fmt(r.verified.introduced)}</td>
        <td>${fmt(r.verified.surviving)}</td>
        <td>${rateCell(r)}</td>
        <td><code class="lb-repro" title="Click to copy" data-repo="${r.repo}">re audit ${r.repo}</code></td>
      </tr>`).join("");

    tbody.addEventListener("click", (e) => {
      const el = e.target.closest(".lb-repro");
      if (el) navigator.clipboard.writeText(`re audit ${el.dataset.repo}`);
    });

    if (data.generated_at) {
      meta.textContent = `Last audited ${new Date(data.generated_at).toUTCString()} · ${rows.length} repositories · powered by the open-source Causari CLI`;
    }
  } catch (e) {
    tbody.innerHTML = '<tr><td colspan="8" style="text-align:center;color:#64748b;">First audit run is in progress — check back soon, or run <code>re audit owner/repo</code> yourself.</td></tr>';
  }
})();
document.getElementById("year").textContent = new Date().getFullYear();
