// A real Docker proof, separate from model-quality claims and unit test doubles.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { verify } from './verify.mjs';

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'crovia-docker-proof-'));
try {
  for (const dir of ['src', 'test', 'resident']) fs.mkdirSync(path.join(root, dir));
  fs.writeFileSync(path.join(root, 'package.json'), '{"type":"module"}');
  fs.writeFileSync(path.join(root, 'src/value.js'), 'export const valid = x => !!x;\n');
  fs.writeFileSync(path.join(root, 'test/unit.test.mjs'), "import assert from 'node:assert/strict'; import { valid } from '../src/value.js'; assert.equal(valid(1), true);\n");
  fs.writeFileSync(path.join(root, 'resident/core.test.mjs'), "import assert from 'node:assert/strict'; assert.equal(1, 1);\n");
  execFileSync('git', ['init', '-q', root]);
  execFileSync('git', ['add', '.'], { cwd: root });
  execFileSync('git', ['-c', 'user.name=Crovia', '-c', 'user.email=info@croviatrust.com', 'commit', '-qm', 'proof baseline'], { cwd: root });
  const episode = {
    id: 'docker-proof', repo: 'croviatrust/agentcrash', status: 'proposed',
    proposal: { title: 'Accept zero', hypothesis: 'Zero is rejected', benefit: 'Handle zero input', falsifier: 'Zero already works', edits: [
      { path: 'src/value.js', content: 'export const valid = x => x !== null;\n' },
      { path: 'resident-regressions/zero.test.mjs', content: "import assert from 'node:assert/strict'; import { valid } from '../src/value.js'; assert.equal(valid(0), true);\n" }
    ] }
  };
  const good = await verify(root, episode);
  assert.equal(good.status, 'verified', JSON.stringify(good));
  console.log('Docker proof: real regression fails before and passes after; original suite passes.');
  episode.proposal.edits[0].content = 'export const valid = x => x === 0;\n';
  const bad = await verify(root, episode);
  assert.equal(bad.status, 'suite-failed', JSON.stringify(bad));
  console.log('Docker proof: a change that breaks existing behavior is rejected.');
} finally { fs.rmSync(root, { recursive: true, force: true }); }
