// Run: node --test --test-concurrency=1 lygos/check-fork-patches.test.mjs
// Set FORK_CHECKER_REV to replay this regression against an older checker.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const source = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const manifestPath = 'lygos/FORK-PATCHES.json';
const checkerPath = 'lygos/check-fork-patches.mjs';
const git = (repo, ...args) => execFileSync('git', ['-C', repo, ...args], { encoding: 'utf8' }).trim();

test('upstream sync history passes while missing patches and false provenance fail closed', () => {
  const scratch = mkdtempSync(path.join(tmpdir(), 'fork-history-'));
  try {
    const repo = path.join(scratch, 'repo');
    execFileSync('git', ['clone', '--shared', '--no-checkout', '--quiet', source, repo]);
    git(repo, 'checkout', 'HEAD', '--', manifestPath, checkerPath);
    const head = git(repo, 'rev-parse', 'HEAD');
    const original = JSON.parse(readFileSync(path.join(source, manifestPath), 'utf8'));
    const write = (m) => writeFileSync(path.join(repo, manifestPath), JSON.stringify(m, null, 2) + '\n');
    const checker = process.env.FORK_CHECKER_REV
      ? git(source, 'show', `${process.env.FORK_CHECKER_REV}:${checkerPath}`)
      : readFileSync(path.join(source, checkerPath), 'utf8');
    writeFileSync(path.join(repo, checkerPath), checker);
    write(original);
    const run = () => spawnSync(process.execPath, [path.join(repo, checkerPath)], { encoding: 'utf8' });
    const pass = (label) => {
      const result = run();
      assert.equal(result.status, 0, `${label}\n${result.stdout}\n${result.stderr}`);
      console.log(`PASS ${label}: ${result.stdout.trim()}`);
    };
    const reject = (label, pattern) => {
      const result = run();
      assert.equal(result.status, 1, `${label}\n${result.stdout}\n${result.stderr}`);
      assert.match(result.stdout, pattern, label);
      console.log(`REJECT ${label}: ${result.stdout.trim()}`);
    };
    pass('complete history');
    const missing = structuredClone(original);
    missing.patches.splice(0, 1);
    write(missing);
    reject('deleted Lygos entry', /manifest lists/);
    write(original);

    // A real new commit is not covered by the upstream anchor, even with an empty tree diff.
    const extra = execFileSync('git', ['-C', repo, '-c', 'user.name=Francis Corvino',
      '-c', 'user.email=francis@lygos.finance', 'commit-tree', `${head}^{tree}`, '-p', head,
      '-m', 'Test an unrecorded change'], { encoding: 'utf8' }).trim();
    git(repo, 'update-ref', 'HEAD', extra);
    reject('unrecorded Lygos commit', /manifest lists/);
    git(repo, 'update-ref', 'HEAD', head);

    for (const commit of ['0'.repeat(40), head]) {
      const bogus = structuredClone(original);
      bogus.upstreamSyncs[0].commit = commit;
      write(bogus);
      reject(`bogus upstream point ${commit}`, /not a reviewed upstream point/);
    }
    const removed = structuredClone(original);
    removed.upstreamSyncs = [];
    write(removed);
    reject('removed upstream point', /upstream sync points differ/);
    const rewritten = structuredClone(original);
    rewritten.upstreamSyncs[0].reason += ' rewritten';
    write(rewritten);
    reject('rewritten upstream record', /changed upstreamSyncs entry/);
    write(original);
    pass('restored history');
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});
