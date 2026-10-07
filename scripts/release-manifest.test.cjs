const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { writeManifest } = require('./release-manifest.cjs');

test('manifest maps both Mac architectures to one universal signed archive and hashes every asset', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'pulse-manifest-'));
  try {
    for (const name of [
      'CodexPulse_1.2.3_x64-setup.exe',
      'CodexPulse_1.2.3_universal.app.tar.gz',
      'CodexPulse_1.2.3_universal.dmg',
    ]) {
      fs.writeFileSync(path.join(directory, name), 'fixture');
      if (!name.endsWith('.dmg'))
        fs.writeFileSync(path.join(directory, name + '.sig'), 'c2lnbmF0dXJl');
    }
    const manifest = writeManifest(directory, '1.2.3');
    assert.deepEqual(Object.keys(manifest.platforms).sort(), [
      'darwin-aarch64',
      'darwin-x86_64',
      'windows-x86_64',
    ]);
    assert.deepEqual(manifest.platforms['darwin-aarch64'], manifest.platforms['darwin-x86_64']);
    assert.match(
      manifest.platforms['windows-x86_64'].url,
      /\/v1\.2\.3\/CodexPulse_1\.2\.3_x64-setup.exe$/,
    );
    assert.equal(
      fs.readFileSync(path.join(directory, 'SHA256SUMS.txt'), 'utf8').trim().split('\n').length,
      6,
    );
    assert.throws(() => writeManifest(directory, '1.2.3-beta.1'));
    fs.unlinkSync(path.join(directory, 'CodexPulse_1.2.3_universal.app.tar.gz.sig'));
    assert.throws(() => writeManifest(directory, '1.2.3'));
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
