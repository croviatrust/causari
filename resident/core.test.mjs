import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { safePath, readFile, validateProposal, applyProposal, initialState, remember, reconcile, chooseFocus, investigate, digest, validateStrategy, materialize, experimentKey } from './core.mjs';
import { verify, commands } from './verify.mjs';
import { publishable } from './github.mjs';

const strategy = JSON.parse(fs.readFileSync(new URL('./strategy.json', import.meta.url)));
const proposal = () => ({ title: 'Handle zero', hypothesis: 'Zero is a valid input', falsifier: 'Zero already works', benefit: 'Avoid false failures', edits: [
  { path: 'src/value.js', content: 'export const valid = x => x !== null;\n' },
  { path: 'resident-regressions/zero.test.mjs', content: "import assert from 'node:assert/strict'; import {valid} from '../src/value.js'; assert.equal(valid(0), true);\n" }
] });
function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'crovia-fixture-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, 'src'));
  fs.writeFileSync(path.join(root, 'src/value.js'), 'export const valid = x => !!x;\n');
  fs.writeFileSync(path.join(root, 'package.json'), '{"type":"module"}');
  execFileSync('git', ['init', '-q', root]);
  execFileSync('git', ['add', '.'], { cwd: root });
  execFileSync('git', ['-c', 'user.name=test', '-c', 'user.email=test@example.com', 'commit', '-qm', 'fixture'], { cwd: root });
  return root;
}
test('rejects traversal, hidden paths and absolute paths', () => {
  for (const p of ['../secret', '/etc/passwd', 'src/../secret', '.github/workflows/x.yml', 'src//x.js', 'src/.env', 'src/x.js\n']) assert.throws(() => safePath(p));
});
test('symlink reads and writes cannot escape the checkout', t => {
  const root = fixture(t);
  const outside = fs.mkdtempSync(path.join(os.tmpdir(), 'crovia-outside-'));
  t.after(() => fs.rmSync(outside, { recursive: true, force: true }));
  fs.writeFileSync(path.join(outside, 'secret'), 'private');
  fs.symlinkSync(outside, path.join(root, 'escape'));
  assert.throws(() => readFile(root, 'escape/secret'));
  assert.throws(() => applyProposal(root, { edits: [{ path: 'escape/secret', content: 'overwrite' }] }));
  assert.equal(fs.readFileSync(path.join(outside, 'secret'), 'utf8'), 'private');
});
test('new source file cannot overwrite a symlink', t => {
  const root = fixture(t);
  fs.symlinkSync('value.js', path.join(root, 'src/link.js'));
  assert.throws(() => applyProposal(root, { edits: [{ path: 'src/link.js', content: 'x' }] }));
});
test('Rust inline tests cannot be silently removed from edited source', t => {
  const root = fixture(t);
  fs.writeFileSync(path.join(root, 'src/example.rs'), 'fn value() {}\n#[cfg(test)]\nmod tests { #[test] fn original() {} }\n');
  assert.throws(() => applyProposal(root, { edits: [{ path: 'src/example.rs', content: 'fn value() {}\n' }] }));
});
test('source edits need regressions and cannot change authority or existing tests', () => {
  const p = proposal();
  assert.equal(validateProposal(p, 'node').kind, 'code');
  assert.throws(() => validateProposal({ ...p, edits: p.edits.slice(0, 1) }, 'node'));
  for (const name of ['package.json', 'resident/core.mjs', 'test/existing.test.mjs', '.github/workflows/ci.yml', 'Cargo.toml'])
    assert.throws(() => validateProposal({ ...p, edits: [{ path: name, content: 'x' }, p.edits[1]] }, 'node'));
});
test('product record modules can be repaired while the resident controller stays protected', () => {
  const p = proposal();
  assert.equal(validateProposal({ ...p, edits: [{ path: 'record/lib/core.js', content: 'export const value = 1;' }, p.edits[1]] }, 'node').kind, 'code');
  assert.throws(() => validateProposal({ ...p, edits: [{ path: 'resident/github.mjs', content: 'changed' }, p.edits[1]] }, 'node'));
});
test('exact replacements avoid repeating large files and reject ambiguous matches', t => {
  const root = fixture(t);
  const p = materialize(root, { edits: [{ path: 'src/value.js', before: '!!x', after: 'x !== null' }] });
  assert.match(p.edits[0].content, /x !== null/);
  assert.throws(() => materialize(root, { edits: [{ path: 'src/value.js', before: 'missing', after: 'x' }] }));
});
test('repeated reads are detected instead of silently filling every turn', async t => {
  const root = fixture(t);
  const answers = [{ action: 'read', path: 'src/value.js' }, { action: 'read', path: 'src/value.js' }, { action: 'stop', reason: 'Need a different experiment' }];
  const result = await investigate({ root, repo: 'croviatrust/agentcrash', state: initialState(), context: {}, strategy,
    ask: async () => ({ text: JSON.stringify(answers.shift()), model: 'test-double' }) });
  assert.ok(result.trace.some(t => t.error?.includes('Already observed')));
  assert.equal(result.reason, 'Need a different experiment');
});
test('rejects duplicate paths, excessive output and null bytes', () => {
  const p = proposal();
  assert.throws(() => validateProposal({ ...p, edits: [...p.edits, p.edits[0]] }, 'node'));
  assert.throws(() => validateProposal({ ...p, edits: [{ ...p.edits[0], content: 'x'.repeat(48001) }, p.edits[1]] }, 'node'));
  assert.throws(() => validateProposal({ ...p, edits: [{ ...p.edits[0], content: '\0' }, p.edits[1]] }, 'node'));
});
test('rust proposals require Rust source and separate integration regressions', () => {
  const p = { ...proposal(), edits: [{ path: 'src/a.rs', content: 'fn main(){}' }, { path: 'tests/resident_a.rs', content: '#[test] fn t(){}' }] };
  assert.equal(validateProposal(p, 'rust').kind, 'code');
  assert.throws(() => validateProposal(proposal(), 'rust'));
  assert.match(commands('rust').regression, /--test resident_/);
});
test('strategy mutation has a fixed schema and is not called verified', async () => {
  validateStrategy(strategy);
  const p = validateProposal({ ...proposal(), edits: [{ path: 'resident/strategy.json', content: JSON.stringify(strategy) }] }, 'node');
  assert.throws(() => validateStrategy({ ...strategy, focus: [] }));
  const result = await verify('.', { id: 's', repo: 'croviatrust/agentcrash', status: 'proposed', proposal: p });
  assert.equal(result.status, 'strategy-candidate');
});
test('focus learns review outcomes without calling tests adoption', () => {
  const state = remember(initialState(), { focus: strategy.focus[0], status: 'verified', pr: 5, proposal: proposal() });
  assert.equal(state.skills[strategy.focus[0]].verified, 1);
  assert.equal(chooseFocus(strategy, state), strategy.focus[1]);
  reconcile(state, [{ number: 5, state: 'closed', merged_at: '2026-11-23' }]);
  reconcile(state, [{ number: 5, state: 'closed', merged_at: '2026-11-23' }]);
  assert.equal(state.skills[strategy.focus[0]].merged, 1);
  assert.equal(state.hypotheses[0], experimentKey(proposal()));
});
test('durable memory index omits source bodies and bounds diagnostic size', () => {
  const state = remember(initialState(), { focus: strategy.focus[0], status: 'verified', proposal: proposal(), error: 'x'.repeat(5000) });
  assert.equal(state.recent[0].proposal.edits, undefined);
  assert.equal(state.recent[0].error.length, 1600);
  assert.equal(state.hypotheses[0], experimentKey(proposal()));
});
test('resident really reads code and produces a validated candidate', async t => {
  const root = fixture(t);
  const answers = [{ action: 'read', path: 'src/value.js' }, { action: 'propose', ...proposal() }];
  const result = await investigate({ root, repo: 'croviatrust/agentcrash', state: initialState(), context: {}, strategy,
    ask: async messages => {
      if (answers.length === 1) assert.match(messages.at(-1).content, /export const valid/);
      return { text: JSON.stringify(answers.shift()), model: 'test-double' };
    } });
  assert.equal(result.status, 'proposed');
  assert.equal(result.proposal.kind, 'code');
});
test('an out-of-range read exposes the actual file length and can be corrected', async t => {
  const root = fixture(t); let call = 0;
  const result = await investigate({ root, repo: 'croviatrust/agentcrash', state: initialState(), context: {}, strategy,
    ask: async messages => {
      call++;
      if (call === 1) return { text: JSON.stringify({ action: 'read', path: 'src/value.js', offset: 99999 }) };
      if (call === 2) {
        assert.match(messages.at(-1).content, /end-of-file.*characters.*offset 0/);
        return { text: JSON.stringify({ action: 'read', path: 'src/value.js', offset: 0 }) };
      }
      return { text: JSON.stringify({ action: 'propose', ...proposal() }) };
    } });
  assert.equal(result.status, 'proposed');
  assert.equal(result.trace[0].offset, 99999);
});
test('resident refuses blind edits and exact repeated experiments', async t => {
  const root = fixture(t);
  const baseSHA = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
  const state = remember(initialState(), { focus: strategy.focus[0], status: 'suite-failed', proposal: proposal(), baseSHA });
  const answers = [{ action: 'propose', ...proposal() }, { action: 'read', path: 'src/value.js' }, { action: 'propose', ...proposal() }, { action: 'stop', reason: 'No new evidence' }];
  const result = await investigate({ root, repo: 'croviatrust/agentcrash', state, context: {}, strategy,
    ask: async () => ({ text: JSON.stringify(answers.shift()), model: 'test-double' }) });
  assert.equal(result.status, 'stopped');
  assert.ok(result.trace.some(t => t.error?.includes('Read src/value.js')));
  assert.ok(result.trace.some(t => t.error?.includes('already attempted')));
});
test('a retained recipe can be reconsidered on a different source revision', async t => {
  const root = fixture(t);
  const state = remember(initialState(), { focus: strategy.focus[0], status: 'stale-base', proposal: proposal(), baseSHA: '0'.repeat(40) });
  const answers = [{ action: 'read', path: 'src/value.js' }, { action: 'propose', ...proposal() }];
  const result = await investigate({ root, repo: 'croviatrust/agentcrash', state, context: {}, strategy,
    ask: async () => ({ text: JSON.stringify(answers.shift()), model: 'test-double' }) });
  assert.equal(result.status, 'proposed');
});
test('unfinished research carries sourced observations into the next generation', async t => {
  const root = fixture(t);
  const answers = [{ action: 'read', path: 'src/value.js' }, { action: 'stop', reason: 'Need a focused test',
    findings: [{ path: 'src/value.js', note: 'The implementation uses truthiness.' }, { path: 'src/invented.js', note: 'Invented' }],
    next: 'Determine whether zero should be a valid value.' }];
  const e = await investigate({ root, repo: 'croviatrust/agentcrash', state: initialState(), context: {}, strategy,
    ask: async () => ({ text: JSON.stringify(answers.shift()), model: 'test-double' }) });
  assert.equal(e.findings.length, 1);
  assert.equal(e.findings[0].sha, e.baseSHA);
  const state = remember(initialState(), e);
  assert.equal(chooseFocus(strategy, state), e.focus);
  assert.equal(state.pendingResearch.question, e.next);
  assert.equal(state.recent[0].findings[0].confidence, 'unverified observation');
});
test('final turns require a decision instead of unlimited additional reading', async t => {
  const root = fixture(t);
  let decisionSeen = false;
  const e = await investigate({ root, repo: 'croviatrust/agentcrash', state: initialState(), context: {}, strategy,
    ask: async messages => {
      if (messages.some(m => m.content.startsWith('Investigation time is over.'))) {
        decisionSeen = true;
        return { text: JSON.stringify({ action: 'stop', reason: 'Need a smaller experiment', next: 'Test the zero input contract.' }) };
      }
      return { text: JSON.stringify({ action: 'read', path: 'src/value.js' }) };
    } });
  assert.ok(decisionSeen);
  assert.equal(e.next, 'Test the zero input contract.');
});
test('verifier demonstrates a real before/after regression on a disposable fixture', async t => {
  const root = fixture(t);
  const p = validateProposal(proposal(), 'node');
  // Only this fixture executor substitutes Docker; it executes the actual
  // regression against the two different source trees, never model code.
  const result = await verify(root, { id: 'fixture', repo: 'croviatrust/agentcrash', status: 'proposed', proposal: p }, {
    run: (dir, command) => {
      if (!command.includes('resident-regressions')) return { code: 0, log: 'fixture suite' };
      try {
        const env = { ...process.env }; delete env.NODE_TEST_CONTEXT;
        const log = execFileSync(process.execPath, ['--test', path.join(dir, 'resident-regressions/zero.test.mjs')], { encoding: 'utf8', stdio: 'pipe', env });
        return { code: 0, log };
      } catch (e) { return { code: e.status, log: e.stdout.toString() }; }
    }
  });
  assert.equal(result.status, 'verified');
  assert.notEqual(result.checks[1].code, 0);
  assert.equal(result.checks[2].code, 0);
  assert.ok(publishable({ id: 'fixture', repo: 'croviatrust/agentcrash', proposal: p }, result));
});
test('an already passing regression or failed suite never becomes verified', async t => {
  const root = fixture(t);
  const episode = { id: 'x', repo: 'croviatrust/agentcrash', status: 'proposed', proposal: proposal() };
  assert.equal((await verify(root, episode, { run: () => ({ code: 0, log: '' }) })).status, 'no-demonstrated-change');
  assert.equal((await verify(root, episode, { run: () => ({ code: 1, log: '' }) })).status, 'baseline-failed');
});
test('timeouts and Docker errors cannot be mistaken for a failing regression', async t => {
  const root = fixture(t);
  let call = 0;
  const result = await verify(root, { id: 'x', repo: 'croviatrust/agentcrash', status: 'proposed', proposal: proposal() }, {
    run: () => ++call === 1 ? { code: 0, log: '' } : { code: null, error: 'timeout', log: '' }
  });
  assert.equal(result.status, 'infrastructure-failed');
});
test('publisher rejects a mismatched proposal or incomplete evidence', () => {
  const e = { id: 'x', repo: 'croviatrust/agentcrash', proposal: proposal() };
  assert.throws(() => publishable(e, { episode: 'y', proposalHash: digest(e.proposal) }));
  assert.throws(() => publishable(e, { episode: 'x', proposalHash: 'wrong' }));
  assert.equal(publishable(e, { episode: 'x', proposalHash: digest(e.proposal), status: 'verified', checks: [] }), false);
});
