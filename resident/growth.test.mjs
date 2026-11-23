import test from 'node:test';
import assert from 'node:assert/strict';
import { discoverOpportunities, rankOpportunities, issueReference, selectOpportunity, growthRecord, feedbackSignals } from './growth.mjs';
import { publicURL } from './research.mjs';
import { runFleet } from './fleet.mjs';
import { fileURLToPath } from 'node:url';
import { initialState, remember, json } from './core.mjs';
const now = () => '2026-11-23T12:00:00.000Z';
const item = (extra = {}) => ({ url: 'https://github.com/example/tool/issues/1', title: 'MCP hook permission problem', excerpt: 'Startup configuration', updated: now(), evidenceHash: 'fixed', ...extra });
test('external opportunity retrieval uses public canonical issues and retains source hashes', async () => {
  const calls = [];
  const report = await discoverOpportunities('croviatrust/agentcrash', {}, { now, get: async url => {
    calls.push(url);
    if (url.includes('/search/issues')) { assert.ok(decodeURIComponent(url).includes('is:public')); return { items: [{ html_url: item().url }] }; }
    if (url.endsWith('/issues/1')) return { state: 'open', html_url: item().url, title: item().title, body: item().excerpt, updated_at: now() };
    return { private: false };
  } });
  assert.equal(report.opportunities.length, 1); const o = report.opportunities[0];
  assert.equal(o.url, item().url); assert.match(o.evidenceHash, /^[a-f0-9]{64}$/); assert.equal(o.retrieved, now());
  assert.ok(calls.every(url => url.startsWith('https://api.github.com/')));
});
test('private sources, pull requests, closed issues and mismatched canonical URLs cannot become opportunities', async () => {
  for (const mode of ['private', 'pr', 'closed', 'mismatch']) {
    let issueFetched = false;
    const r = await discoverOpportunities('croviatrust/agentcrash', {}, { now, get: async url => {
      if (url.includes('/search/issues')) return { items: [{ html_url: item().url }] };
      if (!url.includes('/issues/')) return { private: mode === 'private' };
      issueFetched = true; return { state: mode === 'closed' ? 'closed' : 'open', html_url: mode === 'mismatch' ? 'https://github.com/other/tool/issues/1' : item().url,
        pull_request: mode === 'pr' ? {} : undefined, title: item().title, updated_at: now() };
    } });
    assert.equal(r.opportunities.length, 0, mode); if (mode === 'private') assert.equal(issueFetched, false);
  }
});
test('ranking excludes own echoes, stale or irrelevant evidence and repeated unchanged dead ends', () => {
  const own = item({ url: 'https://github.com/croviatrust/agentcrash/issues/1' });
  const irrelevant = item({ url: 'https://github.com/other/tool/issues/1', title: 'Button color', excerpt: '' });
  const stale = item({ url: 'https://github.com/other/tool/issues/2', updated: '2020-01-01' });
  const boilerplate = item({ url: 'https://github.com/other/tool/issues/3', title: 'Database storage exhausted', excerpt: 'MCP hook security startup permission' });
  const candidates = [item(), item(), own, irrelevant, stale, boilerplate];
  assert.equal(rankOpportunities(candidates, 'croviatrust/agentcrash', {}, Date.parse(now())).length, 1);
  const state = { recent: [1, 2, 3].map(() => ({ opportunity: { url: item().url, evidenceHash: 'fixed' } })) };
  assert.equal(rankOpportunities([item()], 'croviatrust/agentcrash', state, Date.parse(now())).length, 0);
  assert.equal(rankOpportunities([item({ evidenceHash: 'changed' })], 'croviatrust/agentcrash', state, Date.parse(now())).length, 1);
});
test('a scout cannot fabricate source evidence and memory retains the exact selected source', () => {
  const opportunity = { ...item(), id: 'opp-example' };
  assert.equal(selectOpportunity({ opportunityId: 'invented' }, { opportunities: [opportunity] }), null);
  const selected = selectOpportunity({ opportunityId: opportunity.id }, { opportunities: [opportunity] });
  const saved = remember(initialState(), { id: 'fixture', opportunity: selected });
  assert.equal(saved.recent[0].opportunity.evidenceHash, 'fixed');
});
test('feedback and verified code are not automatically classified as adoption or publication', () => {
  const signals = feedbackSignals([{ number: 1, title: '[Agent trial] Used again', body: 'self-report' }, { title: 'Unrelated issue' }]);
  assert.equal(signals.length, 1); assert.match(signals[0].evidenceLevel, /unverified/);
  const r = growthRecord({ id: 'fixture', opportunity: item() }, { status: 'verified' });
  assert.equal(r.adoption, 'unmeasured'); assert.equal(r.publication, 'unpublished');
});
test('GitHub evidence policy permits issue reads but refuses write/account/local destinations', () => {
  for (const u of ['https://127.0.0.1/issues/1', 'https://github.com/a/b/pull/1', 'https://github.com/a/b/issues/1?token=x']) assert.throws(() => issueReference(u));
  assert.equal(publicURL('https://api.github.com/search/issues?q=x', 'github').hostname, 'api.github.com');
  assert.equal(publicURL('https://api.github.com/repos/a/b/issues/1', 'github').hostname, 'api.github.com');
  assert.throws(() => publicURL('https://api.github.com/user/issues', 'github'));
});
test('evidence outages return explicit errors without inventing needs', async () => {
  const r = await discoverOpportunities('croviatrust/causari', {}, { now, get: async () => { throw new Error('rate limited'); } });
  assert.equal(r.opportunities.length, 0); assert.equal(r.errors.length, 2);
});

test('the engineering mission receives actual source candidates and preserves only the selected supplied id', async () => {
  const source = { ...item(), id: 'opp-known' }, root = fileURLToPath(new URL('../', import.meta.url));
  let sawSource = false;
  const episode = await runFleet({ root, repo: 'croviatrust/agentcrash', state: initialState(), context: { opportunities: [source] },
    strategy: json(new URL('./strategy.json', import.meta.url)), study: async () => ({ sources: [], errors: [] }),
    ask: async (messages, { role }) => {
      if (role === 'scout') { sawSource = JSON.stringify(messages).includes(source.url); return { text: JSON.stringify({ opportunityId: source.id, mission: 'Reproduce the reported MCP configuration obstacle' }) }; }
      return { text: role === 'critic' ? '{"decision":"revise","reason":"Reproduction still needed"}' : '{"action":"stop","reason":"Need to reproduce source evidence"}' };
    } });
  assert.ok(sawSource); assert.equal(episode.opportunity.url, source.url); assert.equal(episode.opportunity.evidenceHash, source.evidenceHash);
});
