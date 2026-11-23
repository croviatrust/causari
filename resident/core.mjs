// Crovia's resident: bounded inference, evidence and proposals. No shell tool.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

export const LIMITS = Object.freeze({ turns: 8, files: 6, bytes: 48000, read: 10000 });
export const digest = value => crypto.createHash('sha256').update(JSON.stringify(value)).digest('hex');
export const experimentKey = (proposal, baseSHA) => `exp-${digest({ baseSHA: baseSHA || null, edits: proposal.edits })}`;
export const json = file => JSON.parse(fs.readFileSync(file, 'utf8'));
export function writeJSON(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, JSON.stringify(value, null, 2) + '\n');
}
export function profile(repo) {
  if (repo === 'croviatrust/agentcrash') return 'node';
  if (repo === 'croviatrust/causari') return 'rust';
  throw new Error('Repository is outside the resident mission');
}
export function safePath(name) {
  if (typeof name !== 'string' || name.length > 200 || !/^[A-Za-z0-9_./-]+$/.test(name) ||
      name.startsWith('/') || name.split('/').some(p => !p || p === '.' || p === '..' || p.startsWith('.')))
    throw new Error('Invalid relative path');
  return name;
}
export function readFile(root, name, offset = 0) {
  safePath(name);
  const resolved = fs.realpathSync(path.join(root, name));
  const realRoot = fs.realpathSync(root) + path.sep;
  if (!resolved.startsWith(realRoot)) throw new Error('File escapes checkout');
  if (!fs.statSync(resolved).isFile() || fs.statSync(resolved).size > 500000) throw new Error('Not a small text file');
  const text = fs.readFileSync(resolved, 'utf8');
  if (text.includes('\0')) throw new Error('Binary file');
  if (!Number.isSafeInteger(offset) || offset < 0) throw new Error('Invalid offset');
  return { path: name, offset, total: text.length, text: text.slice(offset, offset + LIMITS.read) };
}
export function tracked(root) {
  return execFileSync('git', ['ls-files', '-z'], { cwd: root, encoding: 'utf8', maxBuffer: 4e6 })
    .split('\0').filter(Boolean).filter(p => !p.split('/').some(x => x.startsWith('.')))
    .sort((a, b) => Number(b.startsWith('src/')) - Number(a.startsWith('src/')) || a.localeCompare(b));
}
export function validateStrategy(value) {
  if (value?.version !== 1 || typeof value.mission !== 'string' || value.mission.length > 1200 ||
      !Array.isArray(value.focus) || value.focus.length < 1 || value.focus.length > 8 ||
      !Array.isArray(value.principles) || value.principles.length > 12 ||
      [...value.focus, ...value.principles].some(s => typeof s !== 'string' || s.length > 1200))
    throw new Error('Invalid strategy');
  return value;
}
export function validateProposal(p, language) {
  for (const key of ['title', 'hypothesis', 'falsifier', 'benefit']) {
    if (typeof p?.[key] !== 'string' || !p[key].trim() || p[key].length > 1800) throw new Error(`Missing or oversized ${key}`);
  }
  if (!Array.isArray(p.edits) || !p.edits.length || p.edits.length > LIMITS.files) throw new Error('Invalid edit count');
  let bytes = 0;
  const seen = new Set();
  for (const edit of p.edits) {
    safePath(edit.path);
    if (seen.has(edit.path)) throw new Error('Duplicate path');
    seen.add(edit.path);
    if (typeof edit.content !== 'string' || edit.content.includes('\0')) throw new Error('Edits must be UTF-8 text');
    bytes += Buffer.byteLength(edit.content);
    const source = language === 'node' ? /^(?:src|record\/lib)\/[A-Za-z0-9_./-]+\.js$/ : /^src\/[A-Za-z0-9_./-]+\.rs$/;
    const regression = language === 'node' ? /^resident-regressions\/[A-Za-z0-9_-]+\.test\.mjs$/ : /^tests\/resident_[A-Za-z0-9_]+\.rs$/;
    if (!source.test(edit.path) && !regression.test(edit.path) && edit.path !== 'resident/strategy.json')
      throw new Error('Edit outside source, new regressions or strategy');
    if (edit.path === 'resident/strategy.json') validateStrategy(JSON.parse(edit.content));
  }
  if (bytes > LIMITS.bytes) throw new Error('Proposal too large');
  const isStrategy = p.edits.length === 1 && p.edits[0].path === 'resident/strategy.json';
  if (!isStrategy && !p.edits.some(e => e.path.startsWith(language === 'node' ? 'resident-regressions/' : 'tests/resident_')))
    throw new Error('Code changes require a new regression test');
  return { ...p, kind: isStrategy ? 'strategy' : 'code' };
}
export function materialize(root, proposal) {
  if (!Array.isArray(proposal.edits)) throw new Error('Missing edits');
  return { ...proposal, edits: proposal.edits.map(edit => {
    safePath(edit.path);
    if (typeof edit.content === 'string') return { path: edit.path, content: edit.content };
    if (typeof edit.before !== 'string' || !edit.before || typeof edit.after !== 'string' || edit.before.length + edit.after.length > 16000)
      throw new Error('Use full content or one bounded before/after replacement');
    // Verify containment even when materializing an edit, before reading it.
    readFile(root, edit.path);
    const original = fs.readFileSync(path.join(root, edit.path), 'utf8');
    if (original.split(edit.before).length !== 2) throw new Error('Replacement must match exactly once');
    return { path: edit.path, content: original.replace(edit.before, () => edit.after) };
  }) };
}
export function applyProposal(root, p) {
  for (const edit of p.edits) {
    safePath(edit.path);
    const target = path.join(root, edit.path);
    let parent = path.dirname(target);
    while (!fs.existsSync(parent)) parent = path.dirname(parent);
    if (fs.realpathSync(parent) !== fs.realpathSync(root) && !fs.realpathSync(parent).startsWith(fs.realpathSync(root) + path.sep))
      throw new Error('Parent escapes checkout');
    if (fs.existsSync(target) && fs.lstatSync(target).isSymbolicLink()) throw new Error('Refusing symlink edit');
    if (edit.path.endsWith('.rs') && fs.existsSync(target)) {
      const original = fs.readFileSync(target, 'utf8');
      const tests = original.indexOf('#[cfg(test)]');
      if (tests !== -1 && !edit.content.endsWith(original.slice(tests))) throw new Error('Existing Rust unit tests must be preserved');
    }
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, edit.content);
  }
}
export function initialState() {
  return { version: 1, generation: 0, recent: [], outcomes: {}, skills: {}, hypotheses: [] };
}
export function remember(state, episode) {
  const next = structuredClone(state);
  next.generation++;
  // Keep the API-readable index small. Full source, traces and test logs live
  // in separate episode files; GitHub's contents API stops embedding large files.
  const compact = e => ({ id: e.id, repo: e.repo, focus: e.focus, started: e.started, finished: e.finished,
    status: e.status, localEvidence: e.localEvidence, reason: e.reason?.slice(0, 1600),
    error: e.error?.slice(0, 1600), pr: e.pr, url: e.url,
    findings: e.findings || [], next: e.next,
    ...(e.proposal ? { proposal: { title: e.proposal.title, hypothesis: e.proposal.hypothesis,
      falsifier: e.proposal.falsifier, benefit: e.proposal.benefit } } : {}) });
  next.recent = [...next.recent, episode].slice(-30).map(compact);
  if (episode.proposal) next.hypotheses = [...new Set([...next.hypotheses, experimentKey(episode.proposal, episode.baseSHA)])].slice(-200);
  const key = episode.focus || 'unknown';
  const skill = next.skills[key] ||= { attempts: 0, verified: 0, merged: 0, rejected: 0 };
  skill.attempts++;
  if (episode.status === 'verified' || episode.localEvidence === 'verified') skill.verified++;
  next.pendingResearch = episode.next ? { focus: episode.focus, question: episode.next,
    attempts: state.pendingResearch?.focus === episode.focus ? state.pendingResearch.attempts + 1 : 1 } : null;
  return next;
}
export function reconcile(state, prs) {
  for (const pr of prs) {
    const previous = state.outcomes[String(pr.number)];
    const outcome = pr.merged_at ? 'merged' : pr.state === 'closed' ? 'rejected' : 'open';
    state.outcomes[String(pr.number)] = outcome;
    const episode = state.recent.find(e => e.pr === pr.number);
    if (episode && previous !== outcome && ['merged', 'rejected'].includes(outcome)) {
      const skill = state.skills[episode.focus];
      if (skill) skill[outcome]++;
    }
  }
  return state;
}
export function chooseFocus(strategy, state) {
  if (state.pendingResearch?.attempts < 3 && strategy.focus.includes(state.pendingResearch.focus)) return state.pendingResearch.focus;
  // Explore untried directions first, then use observed review outcomes.
  return strategy.focus.map((focus, i) => {
    const s = state.skills[focus] || { attempts: 0, merged: 0, rejected: 0 };
    const score = (s.merged - s.rejected) / (s.attempts + 1) + Math.sqrt(2 * Math.log(state.generation + 2) / (s.attempts + 1));
    return { focus, score, i };
  }).sort((a, b) => b.score - a.score || a.i - b.i)[0].focus;
}
export function activeResearch(state, focus) {
  const pending = state.pendingResearch;
  return pending?.attempts < 3 && pending.focus === focus ? pending : null;
}
export async function infer(messages, env = process.env) {
  const remote = Boolean(env.RESIDENT_API_URL);
  const url = remote ? new URL(env.RESIDENT_API_URL) : new URL('http://127.0.0.1:11434/api/chat');
  if (remote && url.protocol !== 'https:') throw new Error('Remote inference requires HTTPS');
  const headers = { 'Content-Type': 'application/json' };
  if (remote && env.RESIDENT_API_KEY) headers.Authorization = `Bearer ${env.RESIDENT_API_KEY}`;
  const model = env.RESIDENT_MODEL || 'qwen3.5:4b';
  const body = remote ? { model, messages, max_tokens: 2400, response_format: { type: 'json_object' } } :
    { model, messages, stream: false, think: false, format: 'json', options: { num_ctx: 8192, num_predict: 2400, temperature: 0.2 } };
  const response = await fetch(url, { method: 'POST', headers, body: JSON.stringify(body), signal: AbortSignal.timeout(200000) });
  if (!response.ok) throw new Error(`Inference HTTP ${response.status}`);
  const data = await response.json();
  return { text: remote ? data.choices?.[0]?.message?.content : data.message?.content,
    usage: remote ? data.usage : { input: data.prompt_eval_count, output: data.eval_count }, model };
}
export async function investigate({ root, sibling, repo, state, context, strategy, ask = infer, turnBudget = LIMITS.turns }) {
  if (!Number.isInteger(turnBudget) || turnBudget < 3 || turnBudget > LIMITS.turns) throw new Error('Invalid investigation budget');
  const language = profile(repo);
  validateStrategy(strategy);
  const focus = chooseFocus(strategy, state);
  const files = tracked(root);
  const messages = [{ role: 'system', content: `You are Crovia, a resident engineering researcher. ${strategy.mission}
Focus: ${focus}
Principles: ${strategy.principles.join(' ')}
Return exactly one JSON object per turn. Allowed actions:
{"action":"read","path":"src/file.js","offset":0,"repository":"self"} (or repository="sibling")
{"action":"search","query":"literal text","repository":"self"} to find text in tracked small files.
{"action":"propose","title":"...","hypothesis":"...","falsifier":"...","benefit":"...","edits":[{"path":"src/file.js","content":"COMPLETE file content"},{"path":"${language === 'node' ? 'resident-regressions/name.test.mjs' : 'tests/resident_name.rs'}","content":"regression test"}]}
{"action":"stop","reason":"specific uncertainty","findings":[{"path":"src/file.js","repository":"self","note":"specific observation from a file you actually read"}],"next":"precise question or experiment for the next generation"}
For an existing file, prefer {"path":"src/file.js","before":"exact unique old text","after":"new text"} instead of repeating the whole file. New files need content. Read every existing file before changing it. Do not read the same path and offset twice: use a new offset, inspect another file, propose or stop. Code edits only under ${language === 'node' ? 'src/ or record/lib/' : 'src/'} with .${language === 'node' ? 'js' : 'rs'} extension. Existing tests, workflows, credentials, package manifests and this controller cannot be edited. New regression tests must fail on the original code and pass after the change. Tests cannot use the network. A strategy experiment may edit only resident/strategy.json and must satisfy the fixed schema; schema validity is not evidence it improves results. No shell, no deletion, no external publishing. Inspect sibling code to discover complementary capabilities but edit only self. ${turnBudget} turns maximum. Prefer one small coherent change.` },
  { role: 'user', content: JSON.stringify({ repo, generation: state.generation, context, files: files.slice(0, 250),
    pendingResearch: activeResearch(state, focus), recent: state.recent.slice(-3).map(e => ({ status: e.status, focus: e.focus,
      title: e.proposal?.title, error: e.error, reason: e.reason, findings: e.findings, next: e.next, pr: e.pr })),
    skills: state.skills }).slice(0, 14000) }];
  const sha = dir => execFileSync('git', ['rev-parse', 'HEAD'], { cwd: dir, encoding: 'utf8' }).trim();
  const episode = { id: `${Date.now()}-${crypto.randomBytes(4).toString('hex')}`, repo, baseSHA: sha(root), siblingSHA: sibling ? sha(sibling) : null, focus, started: new Date().toISOString(), status: 'stopped', trace: [] };
  const reads = new Set();
  const actions = new Set();
  const evidence = new Map();
  for (let turn = 0; turn < turnBudget; turn++) {
    if (turn === turnBudget - 2) messages.push({ role: 'user', content: 'Investigation time is over. The final two turns are reserved for a decision. Return propose with a concrete falsifiable change, or stop with evidence-linked findings and a precise next question. Do not read or search again. An honest unresolved question is better than an invented improvement.' });
    let answer;
    try {
      const response = await ask(messages);
      episode.trace.push({ turn, model: response.model, usage: response.usage });
      answer = JSON.parse(response.text);
      episode.trace.at(-1).action = answer.action;
      if (answer.path) episode.trace.at(-1).path = String(answer.path).slice(0, 200);
      if (answer.action === 'read') episode.trace.at(-1).offset = answer.offset || 0;
      messages.push({ role: 'assistant', content: response.text });
      if (answer.action === 'stop') {
        episode.reason = String(answer.reason).slice(0, 2000);
        episode.findings = (Array.isArray(answer.findings) ? answer.findings : []).slice(0, 3).flatMap(f => {
          const source = evidence.get(`${f.repository || 'self'}:${f.path}`);
          return source && typeof f.note === 'string' ? [{ ...source, note: f.note.slice(0, 500), confidence: 'unverified observation' }] : [];
        });
        if (typeof answer.next === 'string' && answer.next.trim()) episode.next = answer.next.slice(0, 1200);
        break;
      }
      if (answer.action === 'propose') {
        const p = validateProposal(materialize(root, answer), language);
        for (const edit of p.edits) {
          if (fs.existsSync(path.join(root, edit.path)) && !reads.has(edit.path)) throw new Error(`Read ${edit.path} before editing it`);
          if (edit.path.startsWith(language === 'node' ? 'resident-regressions/' : 'tests/resident_') && fs.existsSync(path.join(root, edit.path))) throw new Error('Regression filename already exists');
        }
        if (state.hypotheses.includes(experimentKey(p, episode.baseSHA))) throw new Error('Exact experiment already attempted on this revision');
        episode.proposal = p; episode.status = 'proposed'; break;
      }
      const selected = answer.repository === 'sibling' ? sibling : root;
      if (turn >= turnBudget - 2) throw new Error('Decision required: propose, or stop with sourced findings and the next question.');
      if (!selected) throw new Error('Sibling unavailable');
      const actionKey = JSON.stringify([answer.action, answer.repository || 'self', answer.path, answer.offset || 0, answer.query]);
      if (actions.has(actionKey)) throw new Error('Already observed this exact input. Use a new offset or file, propose an experiment, or stop.');
      actions.add(actionKey);
      let result;
      if (answer.action === 'read') {
        result = readFile(selected, answer.path, answer.offset || 0);
        if (!result.text) throw new Error(`Offset is past the end-of-file (${result.total} characters). Read this file at offset 0, or inspect another file.`);
        evidence.set(`${answer.repository || 'self'}:${answer.path}`, { path: answer.path,
          repository: answer.repository || 'self', sha: selected === root ? episode.baseSHA : episode.siblingSHA });
        if (selected === root) reads.add(answer.path);
      } else if (answer.action === 'search') {
        if (typeof answer.query !== 'string' || answer.query.length < 2 || answer.query.length > 100) throw new Error('Invalid search');
        result = [];
        for (const name of tracked(selected).slice(0, 500)) {
          try { const file = readFile(selected, name); if (file.text.includes(answer.query)) result.push({ path: name, excerpt: file.text.slice(Math.max(0, file.text.indexOf(answer.query) - 200), file.text.indexOf(answer.query) + 600) }); } catch {}
          if (result.length === 8) break;
        }
      } else throw new Error('Unknown action');
      messages.push({ role: 'user', content: JSON.stringify({ evidence: result, turnsLeft: turnBudget - turn - 1 }) });
    } catch (error) {
      episode.trace.push({ turn, error: error.message });
      messages.push({ role: 'user', content: JSON.stringify({ error: error.message, instruction: 'Correct the action or stop honestly.' }) });
      if (episode.trace.filter(t => t.error?.startsWith('Inference HTTP')).length >= 2) {
        episode.reason = 'Inference unavailable; preserve the failed attempt and retry next cycle.'; break;
      }
    }
    // Keep the immutable mission, initial snapshot and most recent evidence.
    while (JSON.stringify(messages).length > 28000 && messages.length > 4) messages.splice(2, 2);
  }
  if (!episode.proposal && !episode.reason) episode.reason = 'Turn budget exhausted without a valid experiment.';
  episode.finished = new Date().toISOString();
  return episode;
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [root, sibling, stateDir, output] = process.argv.slice(2);
  const state = fs.existsSync(path.join(stateDir, 'state.json')) ? json(path.join(stateDir, 'state.json')) : initialState();
  const context = fs.existsSync(path.join(stateDir, 'context.json')) ? json(path.join(stateDir, 'context.json')) : {};
  const strategy = json(path.join(root, 'resident/strategy.json'));
  const episode = await investigate({ root, sibling, repo: process.env.GITHUB_REPOSITORY, state, context, strategy });
  writeJSON(output, episode);
  console.log(JSON.stringify({ id: episode.id, status: episode.status, title: episode.proposal?.title, reason: episode.reason }));
}
