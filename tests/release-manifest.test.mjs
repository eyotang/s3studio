import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, rm, writeFile, readFile, readdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import { execFile } from 'node:child_process';
import { appConfig } from '../scripts/app-config.mjs';
const run = promisify(execFile);
const extensions = ['.exe', '.msi', '.dmg', '.deb', '.AppImage', '.pkg.tar.zst'];
for (const empty of [false, true]) {
  test(empty ? 'release rejects empty installers' : 'release needs no signing key and publishes matching SHA-256 values', async () => {
    const dir = await mkdtemp(path.join(tmpdir(), 's3studio-release-'));
    try {
      const input = path.join(dir, 'input'), output = path.join(dir, 'output');
      await mkdir(input);
      for (const ext of extensions) await writeFile(path.join(input, `app_${appConfig.version}${ext}`), empty ? '' : `installer ${ext}`);
      const build = () => run(process.execPath, ['scripts/prepare-release.mjs', input, output], {
        env: { ...process.env, GITHUB_REF_NAME: `v${appConfig.version}`, S3_STUDIO_UPDATE_KEY: '' },
      });
      if (empty) { await assert.rejects(build, /Empty package/); return; }
      await build();
      assert.equal((await readdir(output)).length, 8);
      const manifest = JSON.parse(await readFile(path.join(output, 'latest.json'), 'utf8'));
      assert.equal(manifest.version, appConfig.version);
      assert.equal(Object.keys(manifest.platforms).length, 3);
      for (const asset of Object.values(manifest.platforms)) {
        assert.equal(asset.signature, undefined);
        assert.ok(asset.url.startsWith(`https://github.com/eyotang/s3studio/releases/download/v${appConfig.version}/`));
        const bytes = await readFile(path.join(output, asset.url.split('/').pop()));
        assert.equal(asset.sha256, createHash('sha256').update(bytes).digest('hex'));
      }
      for (const line of (await readFile(path.join(output, 'SHA256SUMS.txt'), 'utf8')).trim().split('\n')) {
        const [checksum, file] = line.split('  ');
        assert.equal(checksum, createHash('sha256').update(await readFile(path.join(output, file))).digest('hex'));
      }
    } finally { await rm(dir, { recursive: true, force: true }); }
  });
}
