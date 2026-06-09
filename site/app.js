// =============================================================
// Causari — landing app.js
// Pure-JS, no build step, no dependencies.
// =============================================================

(() => {
  // Year in footer
  const yEl = document.getElementById('year');
  if (yEl) yEl.textContent = new Date().getFullYear();

  // ---------------------------------------------------------- Theme
  const THEME_KEY = 'causari.theme';
  const root = document.documentElement;
  const stored = localStorage.getItem(THEME_KEY);
  if (stored === 'light' || stored === 'dark') {
    root.setAttribute('data-theme', stored);
  } else if (window.matchMedia('(prefers-color-scheme: light)').matches) {
    root.setAttribute('data-theme', 'light');
  }
  const toggle = document.getElementById('theme-toggle');
  if (toggle) {
    toggle.addEventListener('click', () => {
      const next = root.getAttribute('data-theme') === 'light' ? 'dark' : 'light';
      root.setAttribute('data-theme', next);
      localStorage.setItem(THEME_KEY, next);
    });
  }

  // ---------------------------------------------------------- Copy
  document.querySelectorAll('.copy-btn').forEach(btn => {
    btn.addEventListener('click', async () => {
      const targetId = btn.dataset.copy;
      const node = document.getElementById(targetId);
      if (!node) return;
      const text = node.innerText;
      try {
        await navigator.clipboard.writeText(text);
        const original = btn.textContent;
        btn.textContent = 'Copied';
        btn.classList.add('copied');
        setTimeout(() => {
          btn.textContent = original;
          btn.classList.remove('copied');
        }, 1400);
      } catch {
        // Fallback selection
        const range = document.createRange();
        range.selectNodeContents(node);
        const sel = window.getSelection();
        sel.removeAllRanges();
        sel.addRange(range);
      }
    });
  });

  // ---------------------------------------------------------- MCP tabs
  const tabs = document.querySelectorAll('.mcp-tab');
  const panels = document.querySelectorAll('.mcp-panel');
  tabs.forEach(tab => {
    tab.addEventListener('click', () => {
      const id = tab.dataset.tab;
      tabs.forEach(t => t.classList.toggle('active', t === tab));
      panels.forEach(p => p.classList.toggle('active', p.dataset.tab === id));
    });
  });

  // ---------------------------------------------------------- Star count
  const starEl = document.getElementById('star-count');
  if (starEl) {
    fetch('https://api.github.com/repos/croviatrust/causari', { cache: 'force-cache' })
      .then(r => r.ok ? r.json() : null)
      .then(j => {
        if (!j) return;
        const n = j.stargazers_count;
        starEl.textContent = n >= 1000 ? (n / 1000).toFixed(1) + 'k' : String(n);
      })
      .catch(() => { /* keep dash */ });
  }

  // ---------------------------------------------------------- Terminal typer
  //
  // Replays a real Causari session captured from the v0.1.0 binary:
  //   $ re init
  //   $ re record --stdin   (rich JSON event)
  //   $ re why src/auth.ts:5
  //
  // The output blocks below are verbatim from the actual CLI; only the
  // line widths were trimmed to fit a 880px-wide terminal viewport.
  // -------------------------------------------------------------------
  const typer = document.getElementById('typer');
  if (typer) {
    // Each step is either a typed command (cmd) or an instant-reveal output.
    // Outputs are arrays of [class, text] segments so we can color them.
    const script = [
      { kind: 'cmd', text: 're churn', after: 360 },
      { kind: 'out', segs: [
        ['t-pass', 'causari churn'],
        ['',       ' — code survival across '],
        ['t-id',   '1,284'],
        ['',       ' events\n\n'],
        ['t-mut',  '  AGENT          INTRO   SURVIVED    WASTE    WASTED $\n'],
        ['',       '  claude-3.5     8,210      6,012    '],
        ['t-warn', '26.8%'],
        ['',       '    '],
        ['t-warn', '$164.10'],
        ['',       '\n  gpt-4o         3,400      1,510    '],
        ['t-red',  '55.6%'],
        ['',       '    '],
        ['t-red',  '$116.90'],
        ['',       '\n  cursor         1,120        980    '],
        ['t-pass', '12.5%'],
        ['',       '      '],
        ['t-pass', '$5.50'],
        ['',       '\n  ' + '─'.repeat(50) + '\n'],
      ], after: 520 },
      { kind: 'out', segs: [
        ['',       '  AI survival '],
        ['t-pass', '66.8%'],
        ['',       '   ·   AI Waste Score '],
        ['t-warn', '33.2%'],
        ['',       '\n  '],
        ['t-red',  '$286.50'],
        ['',       ' of $866.90 spent on code that did not survive'],
      ], after: 1000 },

      { kind: 'cmd', text: 're report --open', after: 240 },
      { kind: 'out', reveal: true, segs: [
        ['t-pass', '✓'],
        ['',       ' report written to '],
        ['t-key',  'causari-report.html'],
        ['',       '  → opening in browser'],
      ], after: 0 },
    ];

    let cancelled = false;
    const sleep = (ms) => new Promise(r => setTimeout(r, ms));

    async function typeChars(node, str, ms) {
      for (let i = 0; i < str.length; i++) {
        if (cancelled) return;
        node.append(str[i]);
        if (ms > 0) await sleep(ms);
      }
    }

    function appendSpan(cls) {
      const s = document.createElement('span');
      if (cls) s.className = cls;
      typer.appendChild(s);
      return s;
    }

    // --- Waste card animation helpers
    const wasteCard   = document.getElementById('waste-card');
    const wcArc       = document.getElementById('wc-arc');
    const wcScore     = document.getElementById('wc-score');
    const wcSurvival  = document.getElementById('wc-survival');
    const wcWasted    = document.getElementById('wc-wasted');

    function resetCard() {
      if (!wasteCard) return;
      wasteCard.classList.remove('show');
      if (wcArc) wcArc.style.strokeDashoffset = '327';
      if (wcScore) wcScore.textContent = '0%';
      if (wcSurvival) wcSurvival.textContent = '0%';
      if (wcWasted) wcWasted.textContent = '$0';
    }

    function countUp(el, to, { prefix = '', suffix = '', duration = 1200, decimals = 1 } = {}) {
      return new Promise((resolve) => {
        const start = performance.now();
        const from = 0;
        function tick(now) {
          const p = Math.min((now - start) / duration, 1);
          const eased = 1 - Math.pow(1 - p, 3);
          const val = from + (to - from) * eased;
          if (decimals > 0) {
            el.textContent = prefix + val.toFixed(decimals) + suffix;
          } else {
            el.textContent = prefix + Math.round(val) + suffix;
          }
          if (p < 1) requestAnimationFrame(tick);
          else resolve();
        }
        requestAnimationFrame(tick);
      });
    }

    async function animateCard() {
      if (!wasteCard) return;
      wasteCard.classList.add('show');
      await sleep(200);
      if (wcArc) wcArc.style.strokeDashoffset = '218'; // ~33.2% waste
      await sleep(200);
      const t1 = countUp(wcScore, 33.2, { suffix: '%' });
      const t2 = countUp(wcSurvival, 66.8, { suffix: '%' });
      const t3 = countUp(wcWasted, 286.50, { prefix: '$', decimals: 2 });
      await Promise.all([t1, t2, t3]);
    }

    async function play() {
      cancelled = false;
      resetCard();
      typer.textContent = '';
      for (const step of script) {
        if (cancelled) return;
        if (step.kind === 'cmd') {
          if (typer.childNodes.length) typer.append('\n');
          appendSpan('t-prompt').textContent = '$ ';
          const cmd = appendSpan('');
          await typeChars(cmd, step.text, 32);
          typer.append('\n');
        } else if (step.kind === 'out') {
          for (const [cls, t] of step.segs) {
            appendSpan(cls).textContent = t;
          }
          typer.append('\n');
          if (step.reveal) await animateCard();
        }
        if (step.after) await sleep(step.after);
      }
    }

    function replay() {
      cancelled = true;
      resetCard();
      setTimeout(play, 50);
    }

    // Start when in viewport
    if ('IntersectionObserver' in window) {
      const io = new IntersectionObserver((entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            play();
            io.disconnect();
            break;
          }
        }
      }, { threshold: 0.3 });
      io.observe(typer);
    } else {
      play();
    }

    // Press R to replay
    document.addEventListener('keydown', (e) => {
      if (e.key === 'r' || e.key === 'R') {
        const ae = document.activeElement;
        if (ae && (ae.tagName === 'INPUT' || ae.tagName === 'TEXTAREA' || ae.isContentEditable)) return;
        replay();
      }
    });
  }
})();
