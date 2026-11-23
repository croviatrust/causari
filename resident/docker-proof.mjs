// A real Docker proof, separate from model-quality claims and unit test doubles.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { runProbe } from './probe.mjs';
import { verify } from './verify.mjs';
import { makeCapsule, communicationDraft } from './learning.mjs';

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'crovia-docker-proof-'));
try {
  for (const dir of ['src', 'test', 'resident', 'resident-regressions']) fs.mkdirSync(path.join(root, dir));
  fs.writeFileSync(path.join(root, 'package.json'), '{"type":"module"}');
  fs.writeFileSync(path.join(root, 'src/value.js'), 'export const valid = x => !!x;\n');
  fs.writeFileSync(path.join(root, 'test/unit.test.mjs'), "import assert from 'node:assert/strict'; import { valid } from '../src/value.js'; assert.equal(valid(1), true);\n");
  fs.writeFileSync(path.join(root, 'resident/core.test.mjs'), "import assert from 'node:assert/strict'; assert.equal(1, 1);\n");
  fs.writeFileSync(path.join(root, 'resident-regressions/legacy.test.mjs'), "import assert from 'node:assert/strict'; import { valid } from '../src/value.js'; assert.equal(valid(-1), true);\n");
  execFileSync('git', ['init', '-q', root]);
  execFileSync('git', ['add', '.'], { cwd: root });
  execFileSync('git', ['-c', 'user.name=Crovia', '-c', 'user.email=info@croviatrust.com', 'commit', '-qm', 'proof baseline'], { cwd: root });
  const episode = {
    id: 'docker-proof', repo: 'croviatrust/agentcrash', status: 'proposed',
    baseSHA: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
    proposal: { title: 'Accept zero', hypothesis: 'Zero is rejected', benefit: 'Handle zero input', falsifier: 'Zero already works', edits: [
      { path: 'src/value.js', content: 'export const valid = x => x !== null;\n' },
      { path: 'resident-regressions/zero.test.mjs', content: "import assert from 'node:assert/strict'; import { valid } from '../src/value.js'; assert.equal(valid(0), true);\n" }
    ] }
  };
  const probe = await runProbe({ root, repo: episode.repo, content: episode.proposal.edits[1].content });
  assert.equal(probe.status, 'baseline-failed', JSON.stringify(probe));
  assert.equal(probe.sourceSHA, episode.baseSHA);
  console.log('Docker proof: investigation receives actual failing baseline diagnostics.');
  const good = await verify(root, episode);
  assert.equal(good.status, 'verified', JSON.stringify(good));
  console.log('Docker proof: real regression fails before and passes after; original suite passes.');
  episode.fleet = { specialist: { id: 'fixture-specialist' }, review: { decision: 'accept' } };
  const capsule = makeCapsule(episode, good);
  assert.ok(capsule && capsule.recipe.edits.length === 2);
  assert.match(communicationDraft(episode, good, capsule), /unpublished draft/);
  console.log('Docker proof: independently executed evidence permits a capsule and a draft.');
  episode.proposal.edits[0].content = 'export const valid = x => x >= 0;\n';
  const bad = await verify(root, episode);
  assert.equal(bad.status, 'regression-failed', JSON.stringify(bad));
  assert.equal(makeCapsule(episode, bad), null);
  console.log('Docker proof: a change that breaks existing behavior is rejected.');
} finally { fs.rmSync(root, { recursive: true, force: true }); }
