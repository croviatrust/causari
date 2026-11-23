import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { reader, publicURL, research } from './research.mjs';
import { runFleet } from './fleet.mjs';
import { evidenceValid, makeCapsule, capsuleIndex, communicationDraft, evolveFleet, selectCapabilities } from './learning.mjs';
import { initialState, digest, remember, json } from './core.mjs';
import { verify } from './verify.mjs';

const strategy = json(new URL('./strategy.json', import.meta.url));
function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'crovia-fleet-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const d of ['src', 'test', 'resident']) fs.mkdirSync(path.join(root, d));
  fs.writeFileSync(path.join(root, 'src/value.js'), 'export const valid = x => !!x;\n');
  fs.writeFileSync(path.join(root, 'package.json'), '{"type":"module"}');
  fs.writeFileSync(path.join(root, 'test/old.test.mjs'), "import assert from 'node:assert/strict'; import {valid} from '../src/value.js'; assert.equal(valid(1), true);\n");
  fs.writeFileSync(path.join(root, 'resident/core.test.mjs'), "import assert from 'node:assert/strict'; assert.ok(true);\n");
  execFileSync('git', ['init', '-q', root]);
  execFileSync('git', ['add', '.'], { cwd: root });
  execFileSync('git', ['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.com', 'commit', '-qm', 'test baseline'], { cwd: root });
  return root;
}
const proposal = () => ({ action: 'propose', title: 'Accept zero values', hypothesis: 'Zero is valid input', falsifier: 'Zero already succeeds', benefit: 'Handle zero without false rejection',
  edits: [{ path: 'src/value.js', before: '!!x', after: 'x !== null' },
    { path: 'resident-regressions/zero.test.mjs', content: "import assert from 'node:assert/strict'; import {valid} from '../src/value.js'; assert.equal(valid(0), true);\n" }] });
const study = async () => ({ sources: [{ repository: 'fixture/reference', revision: 'a'.repeat(40), license: 'MIT', sha256: 'b'.repeat(64),
  url: 'https://github.com/fixture/reference', excerpt: 'Fixture reference material, not a live external source.' }], errors: [] });
async function episode(t, decision = 'accept') {
  const root = fixture(t), roles = [];
  const actions = [{ action: 'read', path: 'src/value.js' }, proposal()];
  const e = await runFleet({ root, repo: 'croviatrust/agentcrash', state: initialState(), context: {}, strategy, study,
    ask: async (messages, { role }) => {
      roles.push(role);
      const value = role === 'scout' ? { mission: 'Repair zero input handling', query: 'fixture' } : role === 'critic' ? { decision, reason: 'Check zero and retain the old value contract.' } : actions.shift();
      return { text: JSON.stringify(value), model: 'test-double', usage: { input: 1, output: 1 } };
    } });
  return { root, e, roles };
}
async function executeFixture(root, e) {
  return verify(root, e, { run: (dir, command) => {
    const env = { ...process.env }; delete env.NODE_TEST_CONTEXT;
    const paths = command.includes('resident-regressions') ? [path.join(dir, 'resident-regressions/zero.test.mjs')] :
      [path.join(dir, 'test/old.test.mjs'), path.join(dir, 'resident/core.test.mjs')];
    try { return { code: 0, log: execFileSync(process.execPath, ['--test', ...paths], { encoding: 'utf8', stdio: 'pipe', env }) }; }
    catch (err) { return { code: err.status, log: String(err.stdout) }; }
  } });
}
test('external evidence cannot target local networks, arbitrary hosts or GitHub account APIs', () => {
  for (const u of ['http://nodejs.org/api', 'https://127.0.0.1/x', 'https://[::1]/x', 'https://evil.example/x', 'https://user:password@nodejs.org/api', 'https://nodejs.org:8443/x']) assert.throws(() => publicURL(u));
  assert.throws(() => publicURL('https://api.github.com/user', 'github'));
  assert.equal(publicURL('https://nodejs.org/api/assert.html').hostname, 'nodejs.org');
});
test('GitHub credentials do not follow a redirect to a documentation host', async () => {
  const calls = [];
  const get = reader({ token: 'fixture-secret', fetcher: async (url, options) => {
    calls.push({ url: String(url), headers: options.headers });
    return new Response('', { status: 302, headers: { location: 'https://nodejs.org/api/' } });
  } });
  await assert.rejects(get('https://api.github.com/repos/example/repo'));
  assert.equal(calls.length, 1);
});
test('documentation fetches carry no GitHub Authorization header', async () => {
  let auth;
  const get = reader({ token: 'fixture-secret', fetcher: async (_, options) => {
    auth = options.headers.Authorization;
    return new Response('public documentation', { headers: { 'content-type': 'text/plain' } });
  } });
  assert.equal((await get('https://nodejs.org/api/', 'documentation')).text, 'public documentation');
  assert.equal(auth, undefined);
});
test('network request and response size budgets are enforced', async () => {
  const get = reader({ maximum: 1, fetcher: async () => new Response('{}', { headers: { 'content-type': 'application/json' } }) });
  await get('https://api.github.com/repos/a/b');
  await assert.rejects(get('https://api.github.com/repos/a/b'), /budget/);
  const large = reader({ fetcher: async () => new Response('x', { headers: { 'content-type': 'application/json', 'content-length': '999999' } }) });
  await assert.rejects(large('https://api.github.com/repos/a/b'), /too large/);
  const doc = reader({ fetcher: async () => new Response('x'.repeat(300000), { headers: { 'content-type': 'text/plain' } }) });
  const partial = await doc('https://nodejs.org/api/', 'documentation');
  assert.equal(partial.text.length, 131072); assert.equal(partial.truncated, true);
});
test('repository evidence is pinned to a commit and retains license and content hash', async () => {
  const calls = [], sha = 'a'.repeat(40);
  const result = await research({ repositories: [{ repository: 'example/reference', path: 'src/a.js' }] }, { get: async url => {
    calls.push(url);
    if (url.endsWith('/example/reference')) return { private: false, default_branch: 'main', license: { spdx_id: 'MIT' } };
    if (url.includes('/commits?')) return [{ sha }];
    return { encoding: 'base64', path: 'src/a.js', content: Buffer.from('reference text').toString('base64') };
  } });
  assert.ok(calls.at(-1).endsWith(`ref=${sha}`));
  assert.equal(result.sources[0].license, 'MIT');
  assert.match(result.sources[0].sha256, /^[a-f0-9]{64}$/);
  assert.ok(result.sources[0].url.includes(sha));
});
test('private sources and malformed repository names are not studied', async () => {
  const result = await research({ repositories: [{ repository: 'example/private' }, { repository: '../bad' }] }, {
    get: async () => ({ private: true }) });
  assert.equal(result.sources.length, 0);
  assert.equal(result.errors.length, 2);
});
test('a generation uses distinct scout, generated specialist and critic contexts', async t => {
  const { e, roles } = await episode(t);
  assert.equal(roles[0], 'scout'); assert.equal(roles.at(-1), 'critic');
  assert.match(roles[1], /^specialist-/);
  assert.equal(e.fleet.modelCalls, 4);
  assert.equal(e.status, 'proposed');
  assert.equal(e.fleet.execution, 'separate sequential model contexts on one runner');
});
test('critic rejection prevents a proposal reaching execution as accepted work', async t => {
  const { e, root } = await episode(t, 'reject');
  assert.equal(e.status, 'review-rejected');
  const verdict = await verify(root, e);
  assert.equal(verdict.status, 'stopped');
  assert.equal(makeCapsule(e, verdict), null);
});
test('fleet model calls stay inside a shared eight-call budget even on repeated errors', async t => {
  const root = fixture(t); let calls = 0;
  const e = await runFleet({ root, repo: 'croviatrust/agentcrash', state: initialState(), context: {}, strategy, study,
    ask: async (_, { role }) => { calls++; return { text: role === 'scout' ? '{}' : role === 'critic' ? '{"decision":"revise","reason":"No valid experiment"}' : '{"action":"unknown"}' }; } });
  assert.equal(calls, 8); assert.equal(e.fleet.modelCalls, 8);
});
test('the real before/after fixture promotes an executable capsule and an evidence-only draft', async t => {
  const { e, root } = await episode(t);
  const verdict = await executeFixture(root, e);
  assert.equal(verdict.status, 'verified');
  const capsule = makeCapsule(e, verdict);
  assert.ok(capsule);
  assert.ok(capsule.recipe.edits.some(x => x.path.startsWith('resident-regressions/')));
  assert.equal(capsule.evidence.transfer, 'not-demonstrated');
  assert.equal(capsule.evidence.sourceRevisions[0].excerpt, undefined);
  const draft = communicationDraft(e, verdict, capsule);
  assert.match(draft, /unpublished draft/); assert.match(draft, /not independently measured/);
  assert.match(draft, /regression-before: exit 1/);
  const state = remember(initialState(), e);
  const fleet = evolveFleet(state, e, verdict, capsule);
  assert.equal(fleet.members.find(m => m.id === e.fleet.specialist.id).verified, 1);
  const index = capsuleIndex(capsule);
  assert.equal(selectCapabilities([index], 'zero input')[0].id, capsule.id);
});
test('model approval, fabricated evidence and infrastructure errors cannot promote skills', async t => {
  const { e } = await episode(t);
  for (const verdict of [
    { episode: e.id, status: 'verified', proposalHash: digest(e.proposal), checks: [] },
    { episode: 'other', status: 'verified', proposalHash: digest(e.proposal), checks: [] },
    { episode: e.id, status: 'stopped', proposalHash: digest(e.proposal), checks: [] }
  ]) { assert.equal(evidenceValid(e, verdict), false); assert.equal(makeCapsule(e, verdict), null); assert.equal(communicationDraft(e, verdict, null), null); }
  assert.equal(makeCapsule({ id: 'stop' }, { episode: 'stop', status: 'stopped', proposalHash: null }), null);
});
test('a retained capsule is available as evidence to a different repository specialist', async t => {
  const { e, root } = await episode(t);
  const capsule = makeCapsule(e, await executeFixture(root, e));
  let received = false;
  await runFleet({ root, repo: 'croviatrust/causari', state: initialState(), context: { siblingCapabilities: [capsuleIndex(capsule)], capabilityTemplates: [capsule] }, strategy, study,
    ask: async (messages, { role }) => {
      if (role === 'scout') return { text: '{"mission":"Handle zero input in Rust"}' };
      if (role === 'critic') return { text: '{"decision":"revise","reason":"No Rust implementation yet"}' };
      received = JSON.stringify(messages).includes(capsule.id);
      return { text: '{"action":"stop","reason":"Need Rust-specific tests","next":"Adapt and verify the zero-input contract in Rust."}' };
    } });
  assert.ok(received);
  // Delivery of a recipe is not successful cross-language transfer.
  assert.equal(capsule.evidence.transfer, 'not-demonstrated');
});
