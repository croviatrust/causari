/* causari.dev — no dependencies, no network. */
(function () {
  "use strict";

  var root = document.documentElement;

  // Theme: follow the system unless the visitor chose; the choice is kept locally.
  var STORE = "causari-theme";
  function apply(theme) {
    if (theme === "light" || theme === "dark") {
      root.setAttribute("data-theme", theme);
    } else {
      root.removeAttribute("data-theme");
    }
    var meta = document.querySelector('meta[name="theme-color"]');
    if (meta) {
      var dark = theme === "dark" || (theme !== "light" && window.matchMedia("(prefers-color-scheme: dark)").matches);
      meta.setAttribute("content", dark ? "#0b0d10" : "#f5f4ef");
    }
  }
  var stored = null;
  try { stored = localStorage.getItem(STORE); } catch (e) { /* private mode */ }
  apply(stored);

  var toggle = document.getElementById("theme-toggle");
  if (toggle) {
    toggle.addEventListener("click", function () {
      var current = root.getAttribute("data-theme");
      var systemDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
      var isDark = current === "dark" || (!current && systemDark);
      var next = isDark ? "light" : "dark";
      apply(next);
      try { localStorage.setItem(STORE, next); } catch (e) { /* ignore */ }
    });
  }

  // Copy buttons: copy the text of the referenced <pre>, comments included.
  document.querySelectorAll("button.copy[data-copy]").forEach(function (btn) {
    btn.addEventListener("click", function () {
      var el = document.getElementById(btn.getAttribute("data-copy"));
      if (!el) return;
      var text = el.innerText.replace(/\s+$/, "") + "\n";
      var done = function () {
        var was = btn.textContent;
        btn.textContent = "copied";
        setTimeout(function () { btn.textContent = was; }, 1200);
      };
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).then(done, done);
      } else {
        var range = document.createRange();
        range.selectNodeContents(el);
        var sel = window.getSelection();
        sel.removeAllRanges();
        sel.addRange(range);
        try { document.execCommand("copy"); } catch (e) { /* ignore */ }
        sel.removeAllRanges();
        done();
      }
    });
  });

  var year = document.getElementById("year");
  if (year) year.textContent = String(new Date().getFullYear());

  // /facts: the weekly numbers come from latest.json, same origin, so the
  // page never states a number the record does not. Without JS or on any
  // failure the placeholders say where to look and the status line says so.
  var facts = document.querySelectorAll("[data-fact]");
  if (facts.length && window.fetch) {
    var status = document.getElementById("facts-status");
    var pct = function (x) { return (Math.round(x * 1000) / 10).toFixed(1) + " %"; };
    fetch("/reports/survival/latest.json", { cache: "no-cache" }).then(function (r) {
      if (!r.ok) throw new Error("HTTP " + r.status);
      return r.json();
    }).then(function (d) {
      var agg = d.aggregate || {};
      var iv = agg.survival_rate_interval_95;
      var values = {
        "title": d.title,
        "date-p": d.date ? " (" + d.date + ")" : "",
        "repositories": agg.repositories != null ? String(agg.repositories) : null,
        "survival": agg.survival_rate != null ? pct(agg.survival_rate) : null,
        "interval": iv ? " (95 % interval " + pct(iv.low) + " to " + pct(iv.high) + ")" : "",
        "doi": d.doi || null
      };
      facts.forEach(function (el) {
        var v = values[el.getAttribute("data-fact")];
        if (v === undefined || v === null) return;
        if (el.getAttribute("data-fact") === "doi" && d.doi) {
          var a = document.createElement("a");
          a.href = "https://doi.org/" + d.doi;
          a.rel = "noopener";
          a.textContent = d.doi;
          el.textContent = "";
          el.appendChild(a);
        } else {
          el.textContent = v;
        }
      });
      if (status) status.textContent = "Live values read from latest.json (" + (d.generated_at || d.date || "") + ").";
    }).catch(function (e) {
      if (status) status.textContent = "latest.json could not be read from this browser (" + e.message + "); the placeholders above say where to look.";
    });
  }
})();
