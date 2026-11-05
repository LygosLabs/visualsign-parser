// Run: node --test --test-concurrency=1 lygos/check-fork-patches.test.mjs
// Set FORK_CHECKER_REV to replay this regression against an older checker.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync, rmSync, mkdirSync } from 'node:fs';
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

// G5: put an unlisted code commit into the reviewed upstream ancestry using
// local history overrides. The merge's combined diff names only the manifest.
for (const mode of ['replace', 'grafts', 'worktree-grafts']) {
  test(`G5 rejects laundering an unlisted code commit through ${mode}`, () => {
    const scratch = mkdtempSync(path.join(tmpdir(), 'fork-g5-'));
    try {
      const clone = path.join(scratch, 'clone');
      execFileSync('git', ['clone', '--shared', '--quiet', source, clone]);
      const repo = mode === 'worktree-grafts' ? path.join(scratch, 'worktree') : clone;
      if (repo !== clone) git(clone, 'worktree', 'add', '--detach', repo, 'HEAD');
      const head = git(repo, 'rev-parse', 'HEAD');
      const upstream = '64b3d79221126646e558b3e1356bfd1544d76773';
      const parent = git(repo, 'rev-parse', `${upstream}^`);
      const checker = process.env.FORK_CHECKER_REV
        ? git(source, 'show', `${process.env.FORK_CHECKER_REV}:${checkerPath}`)
        : readFileSync(path.join(source, checkerPath), 'utf8');
      const manifest = readFileSync(path.join(source, manifestPath), 'utf8');
      const env = { ...process.env };
      delete env.GIT_NO_REPLACE_OBJECTS;
      const run = () => spawnSync(process.execPath, [path.join(repo, checkerPath)], {
        encoding: 'utf8', env,
      });
      writeFileSync(path.join(repo, checkerPath), checker);
      writeFileSync(path.join(repo, manifestPath), manifest);
      assert.equal(run().status, 0, 'complete history control');
      const evilPath = 'src/chain_parsers/visualsign-bitcoin/src/evil.rs';
      writeFileSync(path.join(repo, evilPath), '// Unlisted code change.\n');
      git(repo, 'add', evilPath);
      const commit = (tree, parents, subject) => git(repo, '-c', 'user.name=Francis Corvino',
        '-c', 'user.email=francis@lygos.finance', 'commit-tree', tree,
        ...parents.flatMap(p => ['-p', p]), '-m', subject);
      const unlisted = commit(git(repo, 'write-tree'), [head], 'Test an unlisted code change');
      const m = JSON.parse(manifest);
      m.notes = (m.notes ?? '') + ' ';
      writeFileSync(path.join(repo, manifestPath), JSON.stringify(m, null, 2) + '\n');
      git(repo, 'add', manifestPath);
      const merge = commit(git(repo, 'write-tree'), [head, unlisted], 'Test a manifest merge');
      git(repo, 'update-ref', 'HEAD', merge);
      assert.equal(git(repo, 'show', '--format=', '--name-only', 'HEAD'), manifestPath);
      assert.equal(git(repo, 'show', `HEAD:${evilPath}`), '// Unlisted code change.');
      const control = run();
      assert.equal(control.status, 1, control.stdout + control.stderr);
      assert.match(control.stdout, /manifest lists/, 'ungrafted attack is rejected');
      if (mode === 'replace') {
        git(repo, 'replace', '--graft', unlisted, parent);
        git(repo, 'replace', '--graft', upstream, parent, unlisted);
        assert.equal(git(repo, 'replace', '-l').split('\n').length, 2);
      } else {
        const grafts = path.resolve(repo, git(repo, 'rev-parse', '--git-path', 'info/grafts'));
        mkdirSync(path.dirname(grafts), { recursive: true });
        writeFileSync(grafts, `${unlisted} ${parent}\n${upstream} ${parent} ${unlisted}\n`);
      }
      const result = run();
      assert.equal(result.status, 1, `${mode} must fail closed\n${result.stdout}\n${result.stderr}`);
      assert.match(result.stdout, mode === 'replace' ? /manifest lists/ : /grafts/);
      console.log(`REJECT G5-${mode}: ${result.stdout.trim()}`);
    } finally {
      rmSync(scratch, { recursive: true, force: true });
    }
  });
}

test('rejects a shallow clone even when it contains all audited fork history', () => {
  const scratch = mkdtempSync(path.join(tmpdir(), 'fork-shallow-'));
  try {
    const repo = path.join(scratch, 'repo');
    execFileSync('git', ['clone', '--quiet', '--depth=300', `file://${source}`, repo]);
    assert.equal(git(repo, 'rev-parse', '--is-shallow-repository'), 'true');
    const checker = process.env.FORK_CHECKER_REV
      ? git(source, 'show', `${process.env.FORK_CHECKER_REV}:${checkerPath}`)
      : readFileSync(path.join(source, checkerPath), 'utf8');
    writeFileSync(path.join(repo, checkerPath), checker);
    writeFileSync(path.join(repo, manifestPath), readFileSync(path.join(source, manifestPath)));
    // The pin and its parent exist; rejection must be for shallow history,
    // not a missing object or missing patch record.
    git(repo, 'rev-parse', '5d18e229d65d4015865ea71dbe22b85578bc4c74^');
    const result = spawnSync(process.execPath, [path.join(repo, checkerPath)], { encoding: 'utf8' });
    assert.equal(result.status, 1, `shallow clone must fail closed\n${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /shallow/);
    console.log(`REJECT shallow clone: ${result.stdout.trim()}`);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});
