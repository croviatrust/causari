// The writer imports only the trusted controller, never candidate code.
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { json, writeJSON, initialState, remember, reconcile, validateProposal, profile, digest, chooseFocus } from './core.mjs';
import { evidenceValid, makeCapsule, capsuleIndex, evolveFleet, selectCapabilities, communicationDraft } from './learning.mjs';

const API = 'https://api.github.com';
export async function api(route, { method = 'GET', body, missing = false } = {}) {
  const response = await fetch(`${API}${route}`, { method,
    headers: { Accept: 'application/vnd.github+json', Authorization: `Bearer ${process.env.GH_TOKEN}`, 'X-GitHub-Api-Version': '2022-11-28', 'Content-Type': 'application/json' },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal: AbortSignal.timeout(30000) });
  if (missing && response.status === 404) return null;
  if (!response.ok) throw new Error(`GitHub ${method} ${route}: ${response.status}`);
  return response.status === 204 ? null : response.json();
}
export async function prepare(repo, output) {
  profile(repo);
  const base = `/repos/${repo}`;
  const [stateFile, prs, issues, commits, meta] = await Promise.all([
    api(`${base}/contents/state.json?ref=crovia-memory`, { missing: true }),
    api(`${base}/pulls?state=all&per_page=50&sort=updated&direction=desc`),
    api(`${base}/issues?state=open&per_page=15`),
    api(`${base}/commits?per_page=5`), api(base)
  ]);
  const state = stateFile ? JSON.parse(Buffer.from(stateFile.content, 'base64').toString('utf8')) : initialState();
  const ours = prs.filter(p => /^crovia\/resident-\d+-[a-f0-9]+$/.test(p.head?.ref || ''));
  for (const episode of state.recent.filter(e => !e.pr && ['awaiting-crovia-identity', 'pr-blocked'].includes(e.status))) {
    const branch = `crovia/resident-${episode.id}`;
    const pr = ours.find(p => p.head.ref === branch);
    if (pr) { episode.pr = pr.number; episode.url = pr.html_url; }
    else if (!await api(`${base}/git/ref/heads/${branch}`, { missing: true })) episode.status = 'discarded';
  }
  reconcile(state, ours);
  const open = ours.filter(p => p.state === 'open');
  const context = {
    issues: issues.filter(i => !i.pull_request).map(i => ({ number: i.number, title: i.title, body: i.body?.slice(0, 2000) })),
    commits: commits.map(c => ({ sha: c.sha, message: c.commit.message.slice(0, 1000) })),
    openProposals: open.map(p => ({ number: p.number, title: p.title, url: p.html_url })),
    openWork: prs.filter(p => p.state === 'open' && !p.head?.ref.startsWith('crovia/resident-')).slice(0, 5)
      .map(p => ({ number: p.number, title: p.title, body: p.body?.slice(0, 1200), url: p.html_url })),
    metrics: { stars: meta.stargazers_count, forks: meta.forks_count, openIssues: meta.open_issues_count, observed: new Date().toISOString(), note: 'Repository attention; not adoption or revenue.' }
  };
  const sibling = repo === 'croviatrust/agentcrash' ? 'croviatrust/causari' : 'croviatrust/agentcrash';
  try {
    const other = await api(`/repos/${sibling}/contents/state.json?ref=crovia-memory`, { missing: true });
    const saved = other ? JSON.parse(Buffer.from(other.content, 'base64').toString('utf8')) : {};
    context.siblingCapabilities = (saved.capabilities || []).slice(-24).map(c => ({ ...c, repository: sibling }));
  } catch (e) { context.siblingMemoryError = e.message; }
  const index = [...(state.capabilities || []), ...(context.siblingCapabilities || [])];
  const question = `${chooseFocus(json(new URL('./strategy.json', import.meta.url)), state)} ${state.pendingResearch?.question || ''}`;
  context.capabilityTemplates = [];
  for (const selected of selectCapabilities(index, question)) {
    try {
      profile(selected.repository);
      const file = await api(`/repos/${selected.repository}/contents/capabilities/${selected.id}.json?ref=crovia-memory`, { missing: true });
      if (file && file.content?.length < 200000) {
        const capsule = JSON.parse(Buffer.from(file.content, 'base64').toString('utf8'));
        if (capsule.id === selected.id && capsule.repository === selected.repository) context.capabilityTemplates.push(capsule);
      }
    } catch (e) { context.capabilityLoadError = e.message; }
  }
  writeJSON(path.join(output, 'state.json'), state);
  writeJSON(path.join(output, 'context.json'), context);
  // Backpressure: improve the proposal process rather than flood the owner.
  const pending = state.recent.filter(e => !e.pr && ['awaiting-crovia-identity', 'pr-blocked'].includes(e.status)).length;
  const active = open.length + pending < 3;
  if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, `active=${active}\n`);
  return { active, open: open.length, pending, generation: state.generation };
}
export function publishable(episode, verdict) {
  if (verdict.episode !== episode.id || verdict.proposalHash !== digest(episode.proposal)) throw new Error('Verdict does not match proposal');
  const p = validateProposal(episode.proposal, profile(episode.repo));
  if (episode.fleet && episode.fleet.review?.decision !== 'accept') return false;
  if (p.kind === 'strategy') return verdict.status === 'strategy-candidate';
  return evidenceValid(episode, verdict);
}
async function commitFiles(repo, branch, files, message, parent) {
  const base = `/repos/${repo}`;
  const commit = await api(`${base}/git/commits/${parent}`);
  const tree = await api(`${base}/git/trees`, { method: 'POST', body: { base_tree: commit.tree.sha,
    tree: files.map(f => ({ path: f.path, mode: '100644', type: 'blob', content: f.content })) } });
  const created = await api(`${base}/git/commits`, { method: 'POST', body: { message: `${message}\n\nSigned-off-by: Crovia <info@croviatrust.com>`, tree: tree.sha, parents: [parent],
    author: { name: 'Crovia', email: 'info@croviatrust.com' } } });
  await api(`${base}/git/refs`, { method: 'POST', body: { ref: `refs/heads/${branch}`, sha: created.sha } });
  return created.sha;
}
export async function publish(repo, stateDir, episodeFile, verdictFile, baseSHA) {
  profile(repo);
  const episode = json(episodeFile), verdict = json(verdictFile);
  if (episode.repo !== repo) throw new Error('Episode repository mismatch');
  const state = json(path.join(stateDir, 'state.json'));
  const context = json(path.join(stateDir, 'context.json'));
  episode.status = verdict.status;
  episode.localEvidence = verdict.status;
  episode.error = verdict.error || verdict.checks?.filter(c => c.code !== 0).map(c => `${c.label}: ${c.log?.slice(-1800)}`).join('\n');
  episode.inference = episode.trace;
  delete episode.trace;
  if (episode.proposal && publishable(episode, verdict)) {
    const meta = await api(`/repos/${repo}`);
    const current = await api(`/repos/${repo}/git/ref/heads/${meta.default_branch}`);
    if (current.object.sha !== baseSHA) {
      episode.status = 'stale-base'; episode.error = 'Default branch moved; re-run against the new base.';
    } else {
      const branch = `crovia/resident-${episode.id}`;
      await commitFiles(repo, branch, episode.proposal.edits, `resident: ${episode.proposal.title.slice(0, 120)}\n\nGenerated by Crovia resident; hypothesis and checks in the pull request.`, baseSHA);
      const body = `## Hypothesis\n${episode.proposal.hypothesis}\n\n## User benefit\n${episode.proposal.benefit}\n\n## Falsifier\n${episode.proposal.falsifier}\n\n## Validation\n${verdict.note}\n\n${(verdict.checks || []).map(c => `- ${c.label}: exit ${c.code}`).join('\n')}\n\nGenerated by **Crovia** resident, generation ${state.generation + 1}. No automatic merge. [Execution](${process.env.GITHUB_SERVER_URL}/${repo}/actions/runs/${process.env.GITHUB_RUN_ID}).`;
      episode.url = `https://github.com/${repo}/compare/${meta.default_branch}...${branch}`;
      if (process.env.CROVIA_PR_IDENTITY === 'true') {
        try {
          const identity = await api('/user');
          if (identity.login.toLowerCase() !== 'croviatrust') throw new Error('PR identity must be croviatrust');
          const pr = await api(`/repos/${repo}/pulls`, { method: 'POST', body: { title: `Crovia: ${episode.proposal.title.slice(0, 180)}`, body, head: branch, base: meta.default_branch, draft: true } });
          episode.pr = pr.number; episode.url = pr.html_url;
        } catch (error) { episode.error = error.message; episode.status = 'pr-blocked'; }
      } else {
        episode.status = 'awaiting-crovia-identity';
        episode.error = 'Verified branch saved with Crovia as commit author. Configure CROVIA_GITHUB_TOKEN for PRs authored by croviatrust.';
      }
    }
  }
  const next = remember(state, episode);
  const capsule = makeCapsule(episode, verdict);
  next.fleet = evolveFleet(state, episode, verdict, capsule);
  next.capabilities = [...(state.capabilities || []).filter(c => c.id !== capsule?.id), ...(capsule ? [capsuleIndex(capsule)] : [])].slice(-24);
  const roster = next.fleet.members.map(m => `| ${m.id} | ${m.role} | ${m.attempts} | ${m.verified} |`).join('\n');
  const recipes = next.capabilities.length ? next.capabilities.map(c => `- [${c.id}](capabilities/${c.id}.json)`).join('\n') : 'No independently verified repair recipe retained yet.';
  const files = [
    { path: 'state.json', content: JSON.stringify(next, null, 2) + '\n' },
    { path: `episodes/${episode.id}.json`, content: JSON.stringify({ ...episode, verdict, context }, null, 2) + '\n' },
    { path: 'README.md', content: `# Crovia resident memory\n\nGeneration: ${next.generation}\n\nLatest: ${episode.status}\n\n${episode.url || 'No pull request produced.'}\n\n## Fleet\n\n| Agent | Role | Attempts | Verified source-case repairs |\n| --- | --- | --- | --- |\n${roster}\n\nThe model contexts run sequentially on one runner. Repair credit is assigned to the specialist, after independent execution.\n\n## Retained repair recipes\n\n${recipes}\n\nTransfer to other cases and business growth are not inferred from these counters. This branch stores evidence and experiments.\n` }
  ];
  if (capsule) files.push({ path: `capabilities/${capsule.id}.json`, content: JSON.stringify(capsule, null, 2) + '\n' });
  const draft = communicationDraft(episode, verdict, capsule);
  if (draft) files.push({ path: `outreach/${episode.id}.md`, content: draft });
  const ref = await api(`/repos/${repo}/git/ref/heads/crovia-memory`, { missing: true });
  if (!ref) {
    // Start the memory branch from an empty tree, not a second source checkout.
    const tree = await api(`/repos/${repo}/git/trees`, { method: 'POST', body: { tree: files.map(f => ({ ...f, mode: '100644', type: 'blob' })) } });
    const commit = await api(`/repos/${repo}/git/commits`, { method: 'POST', body: { message: 'resident: first memory', tree: tree.sha, parents: [], author: { name: 'Crovia', email: 'info@croviatrust.com' } } });
    await api(`/repos/${repo}/git/refs`, { method: 'POST', body: { ref: 'refs/heads/crovia-memory', sha: commit.sha } });
  } else {
    const parent = await api(`/repos/${repo}/git/commits/${ref.object.sha}`);
    const tree = await api(`/repos/${repo}/git/trees`, { method: 'POST', body: { base_tree: parent.tree.sha, tree: files.map(f => ({ ...f, mode: '100644', type: 'blob' })) } });
    const commit = await api(`/repos/${repo}/git/commits`, { method: 'POST', body: { message: `resident: generation ${next.generation} (${episode.status})`, tree: tree.sha, parents: [ref.object.sha], author: { name: 'Crovia', email: 'info@croviatrust.com' } } });
    await api(`/repos/${repo}/git/refs/heads/crovia-memory`, { method: 'PATCH', body: { sha: commit.sha, force: false } });
  }
  if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, `## Crovia resident\nGeneration ${next.generation}: **${episode.status}**\n\n${episode.url || episode.error || episode.reason || 'No verified proposal.'}\n`);
  return { generation: next.generation, status: episode.status, url: episode.url };
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [command, ...args] = process.argv.slice(2);
  console.log(JSON.stringify(await (command === 'prepare' ? prepare(...args) : publish(...args))));
}
