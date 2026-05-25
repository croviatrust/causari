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
  // Simulates: $ re why src/auth.ts:42  → rich answer
  const typer = document.getElementById('typer');
  if (typer) {
    const lines = [
      { t: '$ ',                    c: 't-prompt', delay: 250 },
      { t: 're why src/auth.ts:42', c: '',         delay: 22 },
      { t: '\n\n',                  c: '',         delay: 320 },
      { t: 'src/auth.ts:42',        c: 't-key',    delay: 0 },
      { t: '  →  introduced by event ', c: '',     delay: 0 },
      { t: 'a3f7b2c9',              c: 't-id',     delay: 0 },
      { t: '\n  agent:    ',        c: '',         delay: 80 },
      { t: 'claude-3.5-sonnet',     c: 't-val',    delay: 0 },
      { t: '\n  prompt:   ',        c: '',         delay: 0 },
      { t: '"Add JWT refresh logic that rotates every 24h"', c: 't-q', delay: 0 },
      { t: '\n  reads:    spec/auth.md, package.json',  c: '', delay: 0 },
      { t: '\n  writes:   src/auth.ts (lines 38-52)',   c: '', delay: 0 },
      { t: '\n  reasoning:',                             c: '', delay: 0 },
      { t: '\n    The spec calls for refresh tokens with a 24h expiry.', c: '', delay: 0 },
      { t: '\n    I extracted the verify() helper to keep the rotation', c: '', delay: 0 },
      { t: '\n    logic testable in isolation.',         c: '', delay: 0 },
    ];

    let cancelled = false;

    async function type(node, str, ms) {
      for (let i = 0; i < str.length; i++) {
        if (cancelled) return;
        node.append(str[i]);
        if (ms > 0) await sleep(ms);
      }
    }
    const sleep = (ms) => new Promise(r => setTimeout(r, ms));

    async function play() {
      cancelled = false;
      typer.textContent = '';
      // First two entries: typed character-by-character to feel alive.
      for (let i = 0; i < lines.length; i++) {
        if (cancelled) return;
        const seg = lines[i];
        const span = document.createElement('span');
        if (seg.c) span.className = seg.c;
        typer.appendChild(span);
        const speed = i < 2 ? 38 : 0; // type only the prompt+command
        await type(span, seg.t, speed);
        if (seg.delay) await sleep(seg.delay);
      }
    }

    function replay() {
      cancelled = true;
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
