// Read-only demand evidence and honest experiment accounting. Never contacts authors.
import { reader, hashText } from './research.mjs';

const POLICIES = {
  'croviatrust/agentcrash': {
    queries: ['"MCP" "security"', '"Claude Code" "hooks"'],
    titleTerms: ['mcp', 'hook', 'prompt injection', 'untrusted', 'agent', 'permission'],
    terms: ['mcp', 'hook', 'prompt injection', 'untrusted', 'permission', 'startup', 'security']
  },
  'croviatrust/causari': {
    queries: ['"AI" "git blame"', '"code provenance"'],
    titleTerms: ['provenance', 'attribution', 'blame', 'ai-generated', 'ai generated', 'audit', 'traceability'],
    terms: ['provenance', 'attribution', 'git blame', 'ai-generated', 'ai generated', 'audit', 'traceability']
  }
};
export function issueReference(value) {
  const u = new URL(value);
  const m = u.pathname.match(/^\/([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)\/issues\/([1-9][0-9]*)$/);
  if (u.origin !== 'https://github.com' || u.username || u.password || u.search || u.hash || !m || m.slice(1, 3).some(p => p === '.' || p === '..')) throw new Error('Public GitHub issue URL required');
  return { repository: `${m[1]}/${m[2]}`, number: Number(m[3]), url: u.href };
}
export function rankOpportunities(items, repo, state = {}, now = Date.now()) {
  const policy = POLICIES[repo]; if (!policy) throw new Error('Unknown growth mission');
  const seen = new Set();
  return items.flatMap(i => {
    try {
      const ref = issueReference(i.url);
      if (ref.repository.toLowerCase().startsWith('croviatrust/') || seen.has(ref.url)) return [];
      seen.add(ref.url);
      const text = `${i.title} ${i.excerpt}`.toLowerCase();
      const matched = policy.terms.filter(t => text.includes(t));
      if (!matched.length || !policy.titleTerms.some(t => String(i.title).toLowerCase().includes(t))) return [];
      const age = (now - Date.parse(i.updated)) / 86400000;
      if (!Number.isFinite(age) || age < -1 || age > 180) return [];
      const attempts = (state.recent || []).filter(e => e.opportunity?.url === ref.url && e.opportunity.evidenceHash === i.evidenceHash).length;
      if (attempts >= 3) return []; // Reconsider when the source evidence changes.
      return [{ ...i, ...ref, id: `opp-${hashText(ref.url).slice(0, 16)}`, matched,
        score: matched.length * 2 + (age <= 30 ? 2 : 0) - attempts * 2,
        evidenceLevel: 'reported open issue; neither validated demand nor endorsement', attempts }];
    } catch { return []; }
  }).sort((a, b) => b.score - a.score || a.url.localeCompare(b.url)).slice(0, 4);
}
export async function discoverOpportunities(repo, state = {}, { get = reader({ token: null, maximum: 14 }), now = () => new Date().toISOString() } = {}) {
  const policy = POLICIES[repo]; if (!policy) throw new Error('Unknown growth mission');
  const report = { observed: now(), opportunities: [], errors: [], policy: 'Public issues only; no posting, contacting, installs or remote code execution.' };
  const candidates = [], publicRepos = new Map();
  for (const query of policy.queries) {
    try {
      const q = `${query} in:title is:issue is:open is:public archived:false`;
      const response = await get(`https://api.github.com/search/issues?q=${encodeURIComponent(q)}&sort=updated&order=desc&per_page=3`);
      for (const i of (response.items || []).slice(0, 3)) {
        const ref = issueReference(i.html_url);
        if (ref.repository.toLowerCase().startsWith('croviatrust/')) continue;
        if (!publicRepos.has(ref.repository)) {
          const metadata = await get(`https://api.github.com/repos/${ref.repository}`);
          publicRepos.set(ref.repository, metadata.private === false);
        }
        if (!publicRepos.get(ref.repository)) continue;
        // Fetch the canonical issue, not only its search excerpt. No private data is persisted.
        const issue = await get(`https://api.github.com/repos/${ref.repository}/issues/${ref.number}`);
        if (issue.pull_request || issue.state !== 'open' || issue.html_url !== ref.url) continue;
        const title = String(issue.title || '').slice(0, 250), excerpt = String(issue.body || '').slice(0, 2400);
        candidates.push({ url: ref.url, title, excerpt, updated: issue.updated_at,
          retrieved: report.observed, evidenceHash: hashText(`${title}\n${excerpt}\n${issue.updated_at}`), query });
      }
    } catch (error) { report.errors.push({ query, error: error.message.slice(0, 300) }); }
  }
  report.opportunities = rankOpportunities(candidates, repo, state, Date.parse(report.observed));
  return report;
}
export function selectOpportunity(plan, context) {
  const candidate = (context.opportunities || []).find(i => i.id === plan.opportunityId);
  if (!candidate) return null;
  return { id: candidate.id, url: candidate.url, title: candidate.title, evidenceHash: candidate.evidenceHash,
    updated: candidate.updated, evidenceLevel: candidate.evidenceLevel };
}
export function growthRecord(episode, verdict) {
  const opportunity = episode.opportunity;
  return { version: 1, episode: episode.id, source: opportunity || null,
    experiment: episode.proposal ? { title: episode.proposal.title, hypothesis: episode.proposal.hypothesis,
      falsifier: episode.proposal.falsifier, intendedBenefit: episode.proposal.benefit } : null,
    outcome: verdict.status, checks: (verdict.checks || []).map(({ label, code }) => ({ label, code })),
    candidate: episode.url || null, next: episode.next || null,
    adoption: 'unmeasured', publication: 'unpublished',
    limits: 'A reported issue is not permission to contact its author, endorsement, demand validation or evidence that our product solves it.' };
}
export function feedbackSignals(issues) {
  // Own inbound issues are reports, never automatically counted as users or successful adoption.
  return (issues || []).filter(i => /\bagent trial\b/i.test(i.title || '')).slice(0, 5)
    .map(i => ({ number: i.number, title: String(i.title).slice(0, 200), body: String(i.body || '').slice(0, 1800), evidenceLevel: 'self-reported inbound feedback; unverified' }));
}
