// Execute only generated tests in a disposable, credential-free baseline snapshot.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { snapshot, docker, commands } from './verify.mjs';
import { digest, profile } from './core.mjs';

export async function runProbe({ root, repo, content }, { run = docker } = {}) {
  if (typeof content !== 'string' || !content.trim() || Buffer.byteLength(content) > 12000 || content.includes('\0')) throw new Error('Probe requires at most 12000 bytes of test source');
  const language = profile(repo), config = commands(language);
  const sourceSHA = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
  const result = { sourceSHA, testHash: digest(content), status: 'infrastructure-failed' };
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'crovia-probe-'));
  try {
    const baseline = path.join(temp, 'baseline'); snapshot(root, baseline);
    const cargo = language === 'rust' ? path.join(temp, 'cargo') : undefined;
    if (cargo) {
      fs.mkdirSync(cargo);
      const fetched = await run(baseline, 'cargo fetch --locked', config.image, { cargo, network: true, timeout: 360000 });
      if (fetched.code !== 0 || fetched.error) return { ...result, ...fetched };
    }
    const id = crypto.randomBytes(12).toString('hex');
    const name = language === 'node' ? `resident-regressions/probe-${id}.test.mjs` : `tests/resident_probe_${id}.rs`;
    fs.mkdirSync(path.dirname(path.join(baseline, name)), { recursive: true });
    fs.writeFileSync(path.join(baseline, name), content, { flag: 'wx' });
    const command = language === 'node' ? `node --test ${name}` : `cargo test --locked --offline --test resident_probe_${id}`;
    const outcome = await run(baseline, command, config.image, { cargo, timeout: 360000 });
    return { ...result, ...outcome, log: String(outcome.log || '').slice(-8000),
      status: outcome.error || outcome.code === null || [125, 126, 127].includes(outcome.code) ? 'infrastructure-failed' : outcome.code === 0 ? 'baseline-passed' : 'baseline-failed',
      note: 'A failing baseline may be an invalid test, import or compiler error; inspect diagnostics. It is not a verified bug.' };
  } finally { fs.rmSync(temp, { recursive: true, force: true }); }
}
