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
      { kind: 'cmd', text: 're init', after: 280 },
      { kind: 'out', segs: [
        ['',       'Initialized causari repository in '],
        ['t-key',  './my-project/.causari'],
      ], after: 700 },

      { kind: 'cmd', text: 're record -m "Add JWT refresh logic that rotates every 24h"', after: 220 },
      { kind: 'out', segs: [
        ['t-pass', 'recorded '],
        ['t-id',   'e112706ec2'],
        ['',       '  Add JWT refresh logic that rotates every 24h'],
      ], after: 900 },

      { kind: 'cmd', text: 're why src/auth.ts:5', after: 420 },
      { kind: 'out', segs: [
        ['t-key',  'src/auth.ts:5'],
        ['',       '\n  '],
        ['t-mut',  "const REFRESH_TTL = '24h';"],
        ['',       '\n\nintroduced by '],
        ['t-id',   'e112706ec2'],
        ['',       '\n  agent:     '],
        ['t-val',  'claude-3.5-sonnet'],
        ['',       '\n  tool:      '],
        ['t-val',  'edit'],
        ['',       '\n  message:   Add JWT refresh logic that rotates every 24h'],
        ['',       '\n\n  prompt:\n    '],
        ['t-q',    '"Add JWT refresh logic that rotates every 24h"'],
        ['',       '\n\n  reasoning:\n    The spec calls for refresh tokens with a 24h expiry.\n    I extracted issueTokens() so the rotation logic stays\n    testable in isolation.'],
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

    async function play() {
      cancelled = false;
      typer.textContent = '';
      for (const step of script) {
        if (cancelled) return;
        if (step.kind === 'cmd') {
          // newline before the next prompt (except the first)
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
        }
        if (step.after) await sleep(step.after);
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
