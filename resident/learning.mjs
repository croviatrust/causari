// Capability promotion depends on independent execution evidence, not model scores.
import { digest, profile, validateProposal } from './core.mjs';

export function evidenceValid(episode, verdict) {
  if (!episode.proposal) return false;
  if (verdict.episode !== episode.id || verdict.proposalHash !== digest(episode.proposal)) return false;
  if (verdict.status !== 'verified' || !Array.isArray(verdict.checks)) return false;
  const proposal = validateProposal(episode.proposal, profile(episode.repo));
  if (proposal.kind !== 'code') return false;
  for (const label of ['baseline-suite', 'regression-before', 'regression-after', 'candidate-suite']) {
    const cells = verdict.checks.filter(c => c.label === label);
    if (cells.length !== 1 || cells[0].error || !Number.isInteger(cells[0].code)) return false;
    const code = cells[0].code;
    if (label === 'regression-before' ? code <= 0 || [125, 126, 127].includes(code) : code !== 0) return false;
  }
  return true;
}
export function makeCapsule(episode, verdict) {
  if (!evidenceValid(episode, verdict)) return null;
  if (!/^[a-f0-9]{40}$/.test(episode.baseSHA || '')) return null;
  if (episode.fleet?.review?.decision !== 'accept') return null;
  const proposal = validateProposal(episode.proposal, profile(episode.repo));
  const sources = (episode.research?.sources || []).map(({ excerpt, ...source }) => source);
  return {
    version: 1, id: `cap-${digest({ repo: episode.repo, edits: proposal.edits }).slice(0, 20)}`,
    repository: episode.repo, baseSHA: episode.baseSHA, episode: episode.id,
    title: proposal.title, contract: { hypothesis: proposal.hypothesis, falsifier: proposal.falsifier, benefit: proposal.benefit },
    recipe: { edits: proposal.edits, language: profile(episode.repo), note: 'Adapt to new code and re-verify. This is an executable tested repair recipe, not trained model weights.' },
    evidence: { status: 'verified-on-source-case', checks: verdict.checks.map(({ label, code }) => ({ label, code })),
      transfer: 'not-demonstrated', sourceRevisions: sources },
    lineage: { parent: 'crovia', specialist: episode.fleet.specialist.id, consulted: episode.fleet.consulted || [] }
  };
}
export function capsuleIndex(capsule) {
  return { id: capsule.id, repository: capsule.repository, title: capsule.title,
    contract: capsule.contract, evidence: capsule.evidence.status, transfer: capsule.evidence.transfer,
    specialist: capsule.lineage.specialist, sourceEpisode: capsule.episode };
}
export function selectCapabilities(catalog, question, maximum = 2) {
  const words = text => new Set(String(text).toLowerCase().match(/[a-z0-9_]{3,}/g) || []);
  const query = words(question);
  const seen = new Set();
  return catalog.filter(c => /^cap-[a-f0-9]{20}$/.test(c.id || '') && !seen.has(`${c.repository}:${c.id}`) && seen.add(`${c.repository}:${c.id}`))
    .map(c => ({ c, score: [...words(`${c.title} ${JSON.stringify(c.contract)}`)].filter(w => query.has(w)).length }))
    .filter(r => r.score > 0).sort((a, b) => b.score - a.score).slice(0, maximum).map(r => r.c);
}
export function evolveFleet(state, episode, verdict, capsule) {
  const fleet = structuredClone(state.fleet || { version: 1, parent: 'crovia', members: [] });
  const specialist = episode.fleet?.specialist;
  if (!specialist) return fleet;
  for (const role of [{ id: 'scout', role: 'research', mission: 'Find external evidence and define the next specialization' },
    specialist, { id: 'critic', role: 'review', mission: 'Challenge the proposed experiment independently' }]) {
    let member = fleet.members.find(m => m.id === role.id);
    if (!member) { member = { ...role, parent: 'crovia', born: state.generation + 1, attempts: 0, verified: 0, lastSeen: 0 }; fleet.members.push(member); }
    member.attempts++; member.lastSeen = state.generation + 1;
    if (capsule && member.id === specialist.id) member.verified++;
  }
  const fixed = fleet.members.filter(m => m.id === 'scout' || m.id === 'critic');
  const children = fleet.members.filter(m => m.id !== 'scout' && m.id !== 'critic')
    .sort((a, b) => b.verified - a.verified || b.lastSeen - a.lastSeen).slice(0, 6);
  fleet.members = [...fixed, ...children];
  fleet.last = { episode: episode.id, status: verdict.status, membersUsed: episode.fleet.membersUsed,
    modelCalls: episode.fleet.modelCalls, note: 'Verified means source-case execution; business value and transfer remain unmeasured.' };
  return fleet;
}
export function communicationDraft(episode, verdict, capsule) {
  if (!capsule || !evidenceValid(episode, verdict)) return null;
  const clean = value => String(value).replace(/[<>\u0000-\u001f]/g, ' ').slice(0, 1800);
  return `# Draft — ${clean(capsule.title)}\n\nAuthor: Crovia\nStatus: unpublished draft\n\n## Hypothesis tested\n${clean(capsule.contract.hypothesis)}\n\n## Intended benefit (not independently measured)\n${clean(capsule.contract.benefit)}\n\n## Reproducible evidence\n${capsule.evidence.checks.map(c => `- ${c.label}: exit ${c.code}`).join('\n')}\n\nRecipe: capabilities/${capsule.id}.json\nSource repository: ${episode.repo}\nBase revision: ${episode.baseSHA}\n\n## Limits\nVerified on the source case. Transfer to other cases, adoption and revenue are not demonstrated. Review the patch, licensing and full test logs before publication.\n`;
}
