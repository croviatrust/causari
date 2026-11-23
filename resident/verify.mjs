// Runs proposals without network, host credentials, Docker socket or write token.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { execFileSync, spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { json, writeJSON, profile, validateProposal, applyProposal, digest } from './core.mjs';

export function snapshot(root, dest) {
  fs.mkdirSync(dest, { recursive: true });
  const archive = `${dest}.tar`;
  try {
    // macOS tar may close stdin after the archive end marker while Node 20
    // is still writing padding, raising EPIPE despite a valid extraction.
    execFileSync('git', ['archive', '--format=tar', `--output=${archive}`, 'HEAD'], { cwd: root });
    execFileSync('tar', ['-xf', archive, '-C', dest]);
  } finally { fs.rmSync(archive, { force: true }); }
}
export function commands(language) {
  if (language === 'node') return {
    image: 'node:22-bookworm',
    full: 'node --test',
    regression: 'node --test resident-regressions/*.test.mjs'
  };
  return { image: 'rust:1.93-bookworm', full: 'cargo test --locked --offline', regression: 'cargo test --locked --offline --test resident_*' };
}
export function docker(root, command, image, { cargo, network = false, timeout = 600000 } = {}) {
  const name = `crovia-check-${process.pid}-${Math.random().toString(16).slice(2)}`;
  const args = ['run', '--rm', '--name', name, '--network', network ? 'bridge' : 'none', '--cap-drop=ALL',
    '--security-opt=no-new-privileges', '--pids-limit=256', '--memory=6g', '--cpus=2', '--read-only',
    '--user', `${process.getuid()}:${process.getgid()}`, '--tmpfs', '/tmp:rw,exec,size=4g,mode=1777',
    '--mount', `type=bind,source=${root},target=/work,readonly`, '-w', '/work', '-e', 'HOME=/tmp'];
  if (cargo) args.push('--mount', `type=bind,source=${cargo},target=/cargo${network ? '' : ',readonly'}`, '-e', 'CARGO_HOME=/cargo', '-e', 'CARGO_TARGET_DIR=/tmp/target');
  args.push(image, 'sh', '-c', command);
  const result = spawnSync('docker', args, { encoding: 'utf8', timeout, maxBuffer: 4e6 });
  // Killing the Docker client alone does not reliably kill the container.
  if (result.error) spawnSync('docker', ['rm', '-f', name], { timeout: 15000 });
  return { code: result.status, error: result.error?.message, log: `${result.stdout || ''}${result.stderr || ''}`.slice(-24000) };
}
export async function verify(root, episode, { run = docker } = {}) {
  const result = { episode: episode.id, status: 'stopped', checks: [], proposalHash: episode.proposal ? digest(episode.proposal) : null };
  if (episode.status !== 'proposed') return result;
  const language = profile(episode.repo);
  const proposal = validateProposal(episode.proposal, language);
  if (proposal.kind === 'strategy') {
    // Schema checking is not an experiment proving an improvement. Publish as
    // a strategy candidate and learn effectiveness only from future outcomes.
    result.status = 'strategy-candidate';
    result.note = 'Schema valid; effectiveness unmeasured. Human review required.';
    return result;
  }
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'crovia-verify-'));
  try {
    const baseline = path.join(temp, 'baseline');
    const candidate = path.join(temp, 'candidate');
    snapshot(root, baseline); snapshot(root, candidate);
    const cargo = language === 'rust' ? path.join(temp, 'cargo') : undefined;
    if (cargo) fs.mkdirSync(cargo);
    const config = commands(language);
    function check(label, dir, command, options = {}) {
      const outcome = run(dir, command, config.image, { cargo, ...options });
      result.checks.push({ label, ...outcome });
      return outcome.code === 0 && !outcome.error;
    }
    if (cargo && !check('fetch-dependencies', baseline, 'cargo fetch --locked', { network: true })) {
      result.status = 'infrastructure-failed'; return result;
    }
    if (!check('baseline-suite', baseline, config.full)) { result.status = 'baseline-failed'; return result; }
    // The original checkout must not contain these new regression files.
    for (const edit of proposal.edits.filter(e => e.path.startsWith(language === 'node' ? 'resident-regressions/' : 'tests/resident_'))) {
      if (fs.existsSync(path.join(baseline, edit.path))) throw new Error('Regression overwrites an existing file');
      applyProposal(baseline, { edits: [edit] });
    }
    const regressionPassesBefore = check('regression-before', baseline, config.regression);
    const before = result.checks.at(-1);
    if (before.error || before.code === null || before.code === 125 || before.code === 126 || before.code === 127) {
      result.status = 'infrastructure-failed'; return result;
    }
    if (regressionPassesBefore) { result.status = 'no-demonstrated-change'; return result; }
    applyProposal(candidate, proposal);
    if (!check('regression-after', candidate, config.regression)) { result.status = 'regression-failed'; return result; }
    if (!check('candidate-suite', candidate, config.full)) { result.status = 'suite-failed'; return result; }
    result.status = 'verified';
    result.note = 'Regression failed before and passed after; fixed baseline test suite passed. This is not a security audit or evidence of adoption.';
    return result;
  } finally { fs.rmSync(temp, { recursive: true, force: true }); }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [root, episodeFile, output] = process.argv.slice(2);
  const episode = json(episodeFile);
  let result;
  try { result = await verify(root, episode); }
  catch (error) { result = { episode: episode.id, status: 'verification-error', error: error.message }; }
  writeJSON(output, result);
  console.log(JSON.stringify({ episode: result.episode, status: result.status }));
}
